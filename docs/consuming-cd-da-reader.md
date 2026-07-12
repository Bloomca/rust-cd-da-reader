# Consuming `cd-da-reader` (1.0)

A guide for downstream projects. It covers the unified read/stream model, sector
formats and auto-detection, the data-track workflow (save → mount → explore),
what is and isn't automatic for Mode 1 vs Mode 2, reading from file/image
backings, and a migration cheat-sheet from the pre-1.0 API.

Everything here is exercised by the crate's `examples/`; each section links the
runnable one.

---

## 1. The shape of the API

Three steps, always in this order:

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_default()?;   // 1. get a drive
let toc = reader.read_toc()?;             // 2. read the table of contents
let pcm = reader.read_track(&toc, 1)?;    // 3. read a track (audio, by default)
```

Opening a drive:

| Call | Use when |
| --- | --- |
| `CdReader::open_default()` | Grab the first drive that has an audio CD. Usually what you want. |
| `CdReader::list_drives()` → `CdReader::open(&drive)` | Let the user choose; each `DriveInfo` has `has_audio_cd`. |
| `CdReader::open_path("disk6" \| "/dev/sr0" \| r"\\.\E:")` | You already know the platform device path. |

The reader owns the open handle and closes it on `Drop`. Use one reader at a
time — CD drives are physical, sequential devices.

---

## 2. One options struct for every read

Blocking reads and streaming reads run over the **same** machinery; the only
difference is which options struct you hand in.

- **Blocking:** [`ReadOptions`] → `read_track_with_options` / `read_sector_range`
- **Streaming:** [`TrackStreamOptions`] → `open_track_stream_with_options`

Both are builders whose defaults read **audio** with the default retry policy, so
you override only what you need:

```rust
use cd_da_reader::{ReadOptions, SectorReadFormat, RetryConfig};

let options = ReadOptions::default()
    .with_format(SectorReadFormat::Mode1Cooked)   // default: Audio
    .with_retry(RetryConfig::default().with_max_attempts(6));

// NOTE: read_track_with_options takes the options by reference.
let data = reader.read_track_with_options(&toc, 3, &options)?;
```

The convenience methods are just defaults over this:

| Convenience | Equivalent |
| --- | --- |
| `read_track(&toc, n)` | `read_track_with_options(&toc, n, &ReadOptions::default())` |
| `open_track_stream(&toc, n)` | `open_track_stream_with_options(&toc, n, TrackStreamOptions::default())` |

`read_track_with_options` validates the requested format against the track type
from the TOC and returns `CdReaderError::TrackFormatMismatch` if, say, you ask for
`Audio` on a data track. `read_sector_range(start_lba, sectors, &options)` is the
low-level escape hatch: it does **no** validation and expects you to supply valid
bounds and a compatible format.

---

## 3. Sector formats

`SectorReadFormat` selects what the drive returns per sector:

| Format | Bytes/sector | What it is |
| --- | --- | --- |
| `Audio` | 2352 | CD-DA PCM (16-bit signed LE, stereo, 44100 Hz). |
| `Mode1Cooked` | 2048 | Mode 1 **user data only** — sync/header/EDC/ECC stripped. This is the filesystem image. |
| `Mode1Raw` | 2352 | Complete Mode 1 sector: sync + header + 2048 user + EDC/ECC. |
| `Mode2Raw` | 2352 | Complete Mode 2 sector. The Mode 2 *form* is per-sector; the payload lives behind an XA subheader. |

`format.sector_size()` returns these numbers, so
`bytes == sectors * format.sector_size()`.

### Auto-detecting a track's format

You rarely need to hard-code the format — ask the drive:

```rust
for track in &toc.tracks {
    let format = reader.detect_track_format(track)?;
    println!("Track #{}: {format:?}", track.number);
}
```

See `examples/detect_track_formats.rs`.

`detect_track_format` resolves to:

- `Audio` for audio tracks (straight from the TOC),
- `Mode1Cooked` for Mode 1 data tracks,
- `Mode2Raw` for Mode 2 data tracks.

It uses MMC `READ TRACK INFORMATION`, and falls back to inspecting one raw sector
if the drive's Data Mode field is inconclusive. If it still can't tell, you get
`CdReaderError::CannotDetectTrackFormat`.

> There is **no** `find_data_track(&toc)` helper — a "data track" is just
> `!track.is_audio`, so the idiom is a plain filter:
> `toc.tracks.iter().find(|t| !t.is_audio)`.

---

## 4. Reading a data track: save → mount → explore

The common case is a mixed-mode / enhanced ("CD-Extra") disc: audio tracks plus a
data track holding an ISO 9660 filesystem with extra files (artwork, videos,
liner notes).

Because `detect_track_format` returns `Mode1Cooked` for such a track, and cooked
Mode 1 is *exactly* the 2048-byte user data per sector, reading the whole track
cooked gives you a byte-for-byte ISO 9660 image you can write to `.iso` and mount:

```rust
use cd_da_reader::{ReadOptions, SectorReadFormat};

let data_track = toc.tracks.iter().find(|t| !t.is_audio)
    .ok_or("no data track on this disc")?;

let format = reader.detect_track_format(data_track)?;    // Mode1Cooked here
assert_eq!(format, SectorReadFormat::Mode1Cooked);

let options = ReadOptions::default().with_format(format);
let image = reader.read_track_with_options(&toc, data_track.number, &options)?;
std::fs::write("disc.iso", &image)?;
```

The blocking read above buffers the whole image in memory. A data track can be
hundreds of MB, so for anything non-trivial prefer the streaming path with the
same `Mode1Cooked` format — pull chunks and write them straight to the file, so
peak memory stays at one chunk. That is exactly what `examples/save_data_track.rs`
does.

Then mount and explore:

| OS | Mount | Unmount |
| --- | --- | --- |
| macOS | `hdiutil attach disc.iso` | `hdiutil detach /Volumes/<name>` |
| Linux | `sudo mount -o loop,ro disc.iso /mnt/cd` | `sudo umount /mnt/cd` |
| Windows | `Mount-DiskImage -ImagePath disc.iso` | `Dismount-DiskImage -ImagePath disc.iso` |

The full runnable version — streaming to disk with a progress line, including the
Mode 2 branch — is `examples/save_data_track.rs`. To verify a data read against
the on-disc ISO structure (sync pattern, `CD001` signature, cooked-equals-raw),
see `examples/read_data_track.rs`.

---

## 5. Mode 1 vs Mode 2 — what's automatic

**Mode 1 is fully handled.** You can read it either way and both are reliable:

- `Mode1Cooked` (2048 B) — the ready-to-mount user data.
- `Mode1Raw` (2352 B) — the complete sector if you want the framing/ECC. The
  user data is `raw[16..16+2048]` (sync 12 + header 4). `detect_track_format`
  returns `Mode1Cooked`, but you can request raw instead:

  ```rust
  let options = ReadOptions::default().with_format(SectorReadFormat::Mode1Raw);
  ```

**Mode 2 is detected but not cooked for you.** Mode 2 mixes Form 1 (2048-byte
payload) and Form 2 (2324-byte payload) sectors, and the form is encoded in an
8-byte **XA subheader** at the front of each sector's user area — it is a
*per-sector* property, so there is no single "cooked" size for the track. The
crate therefore exposes Mode 2 only as `Mode2Raw` (complete 2352-byte sectors)
and leaves payload extraction to you: read raw, and for each sector inspect the
subheader to decide Form 1 vs Form 2 and slice the payload accordingly. This is
intentionally the consumer's responsibility.

---

## 6. Streaming

Same formats and retry config, pulled incrementally — for live playback or
progress reporting instead of one big blocking read:

```rust
use cd_da_reader::TrackStreamOptions;

let options = TrackStreamOptions::default()
    .with_sectors_per_chunk(27);          // ~64 KB of audio per chunk
let mut stream = reader.open_track_stream_with_options(&toc, 1, options)?;

while let Some(chunk) = stream.next_chunk()? {
    // chunk length == sectors_this_chunk * format.sector_size()
}
```

`TrackStream` also exposes `total_sectors()`, `current_sector()`,
`current_seconds()`, `total_seconds()`, `seek_to_sector()`, and
`seek_to_seconds()`. See `examples/stream_with_progress.rs` and
`examples/stream_last_track.rs`.

---

## 7. Reading from a file/image instead of a drive

Everything above a raw sector read — the `Toc`/`Track` types, track-bounds math
(including the CD-Extra trailing-gap rule), and WAV wrapping — is
hardware-independent. Implement [`AudioSectorReader`] for any backing that can
yield raw CD-DA sectors (a CHD image, a BIN/CUE dump, an in-memory buffer, a
network stream) and reuse the same machinery — **the crate takes on no
image-format dependencies**:

```rust
use cd_da_reader::{AudioSectorReader, Toc, Track, create_wav, lba_to_msf, read_track};

struct MyImage { /* ... */ }

impl AudioSectorReader for MyImage {
    type Error = std::io::Error;
    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
        // decode from your image and return exactly count * 2352 bytes
        // (16-bit signed LE, stereo)
        todo!()
    }
}

// Build a Toc from the image's own metadata (lba_to_msf fills start_msf),
// then read tracks with the free `read_track`:
let pcm = read_track(&image, &toc, 1)?;   // Result<Vec<u8>, TrackReadError<E>>
let wav = create_wav(pcm);                // free fn; also CdReader::create_wav
```

`CdReader` itself implements `AudioSectorReader`, so drive-backed and file-backed
code can share the generic `read_track` path. `read_track` returns
`TrackReadError<E>`, which keeps a bad track request (`Toc`) separate from a
failure inside your backing (`Backend(E)`), preserving your error type. Full
dependency-free example: `examples/file_backend.rs`.

---

## 8. Errors

`CdReaderError` is the one error type for drive operations:

| Variant | Meaning |
| --- | --- |
| `Io(std::io::Error)` | OS/transport failure (open, ioctl, FFI). |
| `Scsi(ScsiError)` | Device reported a SCSI failure; carries status + sense. |
| `Parse(String)` | Couldn't parse a command response. |
| `TrackFormatMismatch { .. }` | Requested a format incompatible with the track type. |
| `CannotDetectTrackFormat { .. }` | `detect_track_format` couldn't determine the format. |
| `NoUsableDrive` | Enumeration found no drive with an audio CD. |

The file-backing `read_track` uses its own `TrackReadError<E>` (see §7) so your
backing's error type isn't flattened into the SCSI-oriented `CdReaderError`.

---

## 9. Migrating from the pre-1.0 API

1.0 renamed a lot for clarity. The mechanical substitutions:

| Before (0.x) | Now (1.0) |
| --- | --- |
| `CdReader::open("disk6")` | `CdReader::open_path("disk6")`, or `CdReader::open(&drive_info)` |
| `SectorReadMode` | `SectorReadFormat` |
| `SectorReadMode::DataCooked` | `SectorReadFormat::Mode1Cooked` |
| `SectorReadMode::DataRaw` | `SectorReadFormat::Mode1Raw` (plus new `Mode2Raw`) |
| `read_data_sectors(lba, n, mode, &cfg)` | `read_sector_range(lba, n, &ReadOptions::default().with_format(fmt).with_retry(cfg))` |
| `read_track_with_retry(&toc, n, &cfg)` | `read_track_with_options(&toc, n, &ReadOptions::default().with_retry(cfg))` |
| `TrackStreamConfig { sectors_per_chunk, retry }` | `TrackStreamOptions::default().with_sectors_per_chunk(..).with_retry(..)` |
| `open_track_stream(&toc, n, cfg)` | `open_track_stream(&toc, n)` or `open_track_stream_with_options(&toc, n, options)` |

New in 1.0: `detect_track_format`, `Mode1Raw` / `Mode2Raw`, per-read
`ReadOptions`, format validation on `read_track_with_options`, and the
`AudioSectorReader` file-backing seam.

[`ReadOptions`]: https://docs.rs/cd-da-reader/latest/cd_da_reader/struct.ReadOptions.html
[`TrackStreamOptions`]: https://docs.rs/cd-da-reader/latest/cd_da_reader/struct.TrackStreamOptions.html
[`AudioSectorReader`]: https://docs.rs/cd-da-reader/latest/cd_da_reader/trait.AudioSectorReader.html
