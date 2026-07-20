# Add file-image CD-DA reading (CHD) to `cd-da-reader`

> **Task prompt** for a Claude Code session working in this repo
> (`rust-cd-da-reader`, crate `cd-da-reader`). Self-contained: follow it top to
> bottom, then run the verification steps before reporting done. Line numbers are
> approximate — `rg` for symbols.

## Goal

Teach `cd-da-reader` to read CD-DA audio from a **file image** — starting with
**CHD** (MAME's compressed disc image) — and return raw PCM in the **exact same
format** the existing physical-drive path already produces: 16-bit **signed
little-endian**, stereo, 2352 bytes/sector. Because the format matches, consumers
reuse the crate's existing `Track`, `Toc`, and `create_wav` unchanged.

Motivating consumer: **ODE-artwork-downloader** wants to play audio tracks
straight from CHD disc images (it already stores games as CHD). Today it has no
way to do this — see "How the consumer will use it" below.

## Current state (what you're extending)

`cd-da-reader` is currently **physical-drive only**:

- `CdReader::open("/dev/sr0" | "disk6" | r"\\.\E:")` → SCSI/ioctl to real hardware.
- `read_toc() -> Toc`, `read_track(&toc, n) -> Vec<u8>` (raw PCM), `read_data_sectors(...)`.
- `Track { number: u8, start_lba: u32, start_msf: (u8,u8,u8), is_audio: bool }`
- `Toc { first_track, last_track, tracks: Vec<Track>, leadout_lba }`
- `SectorReadMode::{Audio (2352 B), DataCooked (2048 B), DataRaw (2352 B)}`
- `create_wav(Vec<u8>)` prepends a 44-byte RIFF header hard-coded to **44100 Hz,
  2 channels, 16-bit** — i.e. it already assumes exactly the CD-DA PCM layout.

A CHD is a **file**, not a device, so none of the SCSI code paths apply. This is a
new, parallel backend — do **not** try to route it through `CdReader`.

CD-DA facts you'll rely on: 44100 Hz · 16-bit signed LE · stereo · **2352
bytes/sector** (= 588 stereo sample-frames) · **75 sectors/sec**.

## Recommended backend: `libchdman-rs` (feature-gated)

Use **`libchdman-rs`** (a Rust wrapper over MAME's `chdman` core). It already
parses CHD, enumerates tracks, decompresses, and — critically — handles the
CD-DA **endian swap** and **subcode strip** correctly. It is the *same core the
consumer (ODE) already links*, so behavior matches and there's no second CHD
implementation to keep in sync.

Keep the default crate lean (today just `libc` / `windows-sys`) by putting all of
this behind a **`chd` cargo feature**:

```toml
[features]
chd = ["dep:libchdman-rs"]

[dependencies]
libchdman-rs = { version = "0.288", features = ["prebuilt"], optional = true }
```

> `prebuilt` downloads a prebuilt static lib so there's no MAME C++ compile.
> `libchdman-rs` is a `links` crate — the whole dependency graph must resolve to a
> **single** copy. The consumer (ODE) pins `0.288.8`; keep this pin
> semver-compatible (`^0.288`) so a downstream `cargo tree -i libchdman-rs` shows
> exactly one version. Bump both together when a new MAME release lands.

### The `libchdman-rs` API you need

```rust
use libchdman_rs::Chd;
use libchdman_rs::cd::{list_tracks, extract_to_cue, TrackType, TrackInfo};

let chd = Chd::open(path_str, /*writeable=*/ false, /*parent=*/ None)?;
let tracks: Vec<TrackInfo> = list_tracks(&chd)?;
//   TrackInfo { track_num: u32, track_type: TrackType, frames: u32, pregap: u32, ... }
//   TrackType::Audio == the CD-DA tracks you care about.

// Robust way to get playable PCM: extract the whole CD to a redump-style BIN/CUE.
let mut on_progress = |_written: u64| {};
extract_to_cue(chd_path, &cue_path, &bin_path, &mut on_progress)?;
```

**Why `extract_to_cue` and not the raw reader:** `libchdman-rs::cd::CdCookedReader`
looks tempting but it **explicitly rejects audio tracks** (it only cooks
Mode1/Mode2 *data* sectors to 2048 bytes). `extract_to_cue`, in contrast, writes
each audio track as **2352-byte little-endian sectors** — it strips the 96-byte
subcode and byte-swaps the audio back to LE, exactly matching
`SectorReadMode::Audio`. It handles every CD CHD, **including audio discs that
stored subcode** (the subcode bytes are dropped, the audio track is kept — verify
this yourself in `libchdman-rs`'s `extract_to_cue`, the older doc comment there
saying "tracks with subcode are dropped" is misleading; the code drops only the
subcode bytes).

### Endian gotcha (do not skip)

CHD stores CD-DA **big-endian byte-swapped** internally. The output you return
**must be little-endian** (WAV / `SectorReadMode::Audio` order). `extract_to_cue`
does this swap for you. If you later add a streaming path via raw
`Chd::read_bytes` (see "Follow-ups"), *you* own the swap (swap each 16-bit sample)
and the 2448→2352 subcode strip.

### Lighter alternative (only if the MAME static lib is a dealbreaker)

The pure-Rust `chd` crate reads CHD without a native lib, but then you implement
track enumeration, per-track frame-offset math, subcode strip, and the endian
swap yourself — more code and more ways to get subtly-wrong audio. Prefer
`libchdman-rs` unless there's a hard reason to avoid the prebuilt static lib.

## Proposed public API

Mirror the physical reader so both feel the same and `Track`/`Toc`/`create_wav`
are reused verbatim:

```rust
#[cfg(feature = "chd")]
pub struct CdImageReader { /* opened image + parsed track metadata (+ temp bin) */ }

#[cfg(feature = "chd")]
impl CdImageReader {
    /// Open a CD image file (CHD for now; BIN/CUE later). Errors if it isn't a
    /// recognized CD image.
    pub fn open(path: &std::path::Path) -> Result<Self, CdImageError>;

    /// Table of contents built from the image's own track metadata.
    pub fn read_toc(&self) -> Result<Toc, CdImageError>;

    /// Raw PCM for one audio track — little-endian 16-bit stereo, 2352 B/sector,
    /// byte-for-byte the same format as `CdReader::read_track`. Errors on a
    /// non-audio track. Wrap with `create_wav` to get a playable file.
    pub fn read_track(&self, toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdImageError>;
}
```

- **Reuse `Track` / `Toc`.** Fill `Track.number` from `track_num`, `is_audio` from
  `track_type == TrackType::Audio`, and `start_lba` / `start_msf` from cumulative
  frame offsets (75 frames/sec for MSF; there's an `msf`-style helper pattern to
  copy). `Toc.leadout_lba` = total frames across all tracks.
- **`create_wav` already fits** (44100/2ch/16-bit) — the returned `Vec<u8>` drops
  straight into it. Add a `chd`-gated test asserting that.
- Add a `CdImageError` in `errors.rs` (or reuse/extend `CdReaderError`) covering
  open failure, non-CD image, track-not-found, and non-audio track.

## Implementation sketch (extract-based; robust first)

1. **`open`**: confirm the file is a CHD (magic `MComprHD` at offset 0, or `.chd`
   extension). `Chd::open` it, `list_tracks`, and stash the track metadata. You
   may lazily `extract_to_cue` into a temp dir on first `read_track` (use
   `std::env::temp_dir()`, or add `tempfile` as a `chd`-only dep) and cache the
   bin path + each track's `(start_sector, sector_count)`.
2. **`read_toc`**: build `Toc` from the stashed metadata — cumulative LBA/MSF,
   `is_audio`, `first_track`/`last_track`, `leadout_lba`.
3. **`read_track`**: for an audio track, read its `frames * 2352` bytes out of the
   extracted BIN (already LE) and return them. Error on non-audio (a CD-DA player
   only needs audio; supporting cooked data reads is optional).
4. **`Drop`**: delete the temp BIN/CUE.

Trade-off to note in a comment: this extracts the **whole disc** to a temp file
before the first read. Fine for correctness and simplicity; see Follow-ups for a
streaming path that avoids it.

## Example + tests + README

- **Example** `examples/read_chd.rs` (model it on the existing
  `read_all_tracks.rs` / `play_audio_track.rs`): open a CHD, print the TOC, and
  dump track 1 to `track1.wav` via `create_wav`. Gate it on the `chd` feature.
- **Tests**: MSF conversion, track enumeration, and a `create_wav`
  round-trip; if you can commit a tiny fixture CHD, assert its TOC. Put
  `chd`-dependent tests behind `#[cfg(feature = "chd")]`.
- **README**: document the `chd` feature and a short usage snippet.

## How the consumer (ODE) will use it

ODE's `audio-chd-player` branch already has an adapter `src/disc/chd_audio.rs`
written against a **nonexistent** `rust_cdda` crate (`CdImage::open`,
`img.audio_reader`, `reader.read_samples`). Once this lands, that adapter is
rewritten to the real API: `CdImageReader::open(path)` → `read_toc()` →
`read_track()`, feeding rodio for playback. ODE will depend on this crate via a
**path dependency** with the feature on:

```toml
cd-da-reader = { path = "../rust-cd-da-reader", features = ["chd"] }
```

Because ODE also depends on `libchdman-rs` directly (`0.288.8`), the pins must be
compatible so only one `links` copy resolves — hence the `^0.288` guidance above.

## Verification (run all; report output)

```sh
cargo build --features chd
cargo build                       # default build stays lean — no libchdman pulled in
cargo test  --features chd
cargo run   --example read_chd --features chd -- /path/to/disc.chd
cargo clippy --features chd -- -D warnings
cargo fmt --check
```

- Play the dumped `track1.wav` and confirm it sounds correct (right pitch/speed →
  proves the endianness is right; garbled/static → the LE swap is wrong).
- From a consumer that pins `libchdman-rs`, confirm `cargo tree -i libchdman-rs`
  shows a **single** version.

## Done criteria

- A `chd` cargo feature adds `CdImageReader` (`open` / `read_toc` / `read_track`),
  returning audio as **little-endian** PCM identical in format to the physical
  `CdReader::read_track`.
- The **default** build is unchanged and pulls in **no** new deps; the `chd` build
  is clean (build, test, clippy, fmt).
- `examples/read_chd.rs` dumps a playable WAV from a real CHD.

## Follow-ups (don't gold-plate; note them, don't build unless asked)

- **Streaming reader** (avoids the whole-disc temp extract): read raw 2448-byte CD
  frames via `Chd::read_bytes`, strip the 96-byte subcode, swap each 16-bit sample
  to LE, and expose a `Read`/iterator like the existing `TrackStream`. You own the
  per-track frame-offset math (chdman pads track lengths) and the endian swap here
  — get it right against the `extract_to_cue` output as a reference.
- **BIN/CUE images** through the same `CdImageReader::open` (parse the cue, read
  audio sectors directly — already LE, no CHD needed).
