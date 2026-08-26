## Rust CD-DA reader

[![Crates.io](https://img.shields.io/crates/v/cd-da-reader.svg)](https://crates.io/crates/cd-da-reader)
[![CI](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml/badge.svg?branch=main)](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml)

This library provides cross-platform audio CD reading capabilities and is tested on Windows, macOS, and Linux. It was written to enable CD ripping, but it can also be used to build a live audio CD player. The primary API reads physical discs; to read from a file, image, or another custom source, implement [`AudioSectorReader`](https://docs.rs/cd-da-reader/latest/cd_da_reader/trait.AudioSectorReader.html) and provide a [`Toc`](https://docs.rs/cd-da-reader/latest/cd_da_reader/struct.Toc.html).

Physical-disc access uses platform CD-drive APIs on macOS and direct SCSI commands on Windows and Linux. The library abstracts both access to the drive and reading the data, so callers do not interact with the hardware directly.

A typical audio CD read happens in this order:

1. Open a CD drive
2. Read the disc's TOC (Table of Contents)
3. Read track data using sector ranges from the TOC

Let's go through each concept in order.

## CD access

First thing, we'll need to get a hold of the CD drive. You can see the drive's letter on Windows in File Explorer (although the actual handle will be something like `"\\.\E:"`), with `cat /proc/sys/dev/cdrom/info` on Linux and with `diskutil list` on macOS.

This is a bit brittle, so this library provides a few helper methods to find a correct CD drive. By far the most straightforward approach is to simply open the "default" drive:

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_default()?;
```

This code will scan the CD drives and will open the first one with an audio CD in it, and _usually_ this is what you want. If you want to provide a choice, there is an additional function to list all drives:

```rust
use cd_da_reader::CdReader;

let drives = CdReader::list_drives()?;
```

This gives you a vector of drives. Each entry has a `has_audio_cd` field and can
be opened directly:

```rust
use cd_da_reader::CdReader;

let drives = CdReader::list_drives()?;
let selected = drives
    .iter()
    .find(|drive| drive.has_audio_cd)
    .ok_or("no drive with an audio CD found")?;
let reader = CdReader::open(selected)?;
```

If you already know the platform-specific device path, use `open_path`:

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_path("disk14")?;
```

## Reading the TOC

Each audio CD carries a Table of Contents containing the location and type of every track. Read it before issuing track-level read commands:

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;
```

This returns a structure like:

```text
{
    first_track: 1,
    last_track: 11,
    tracks: [{
        number: 1,
        start_lba: 0,
        start_msf: (0, 2, 0),
        is_audio: true,
    }, {
        number: 2,
        start_lba: 14675,
        start_msf: (3, 17, 50),
        is_audio: true,
    }, ...],
    leadout_lba: 221786
}
```

Each track's `number` comes from the disc. It is not a zero-based index into `toc.tracks`, and track numbers are not guaranteed to begin at 1. Select tracks using their metadata—such as `is_audio`—and pass the reported `number` to the read APIs.

Each track also has two equivalent address fields:

- **LBA (Logical Block Address):** a sequential sector index used internally for read commands. LBA 0 corresponds to the first program-area sector at MSF `(0, 2, 0)`.
- **MSF (Minutes:Seconds:Frames):** a time-based address inherited from the physical disc layout. One frame is one sector, and there are 75 frames per second. MSF includes the standard 150-frame offset, so `(0, 2, 0)` corresponds to LBA 0.

The two are interchangeable: `LBA + 150 = total MSF frames`. Most callers only need the track number, while LBA is useful for custom sector ranges and MSF is required by services such as MusicBrainz disc ID calculation.

## Reading tracks

Pass the TOC and a track's reported number to `CdReader::read_track`. The library calculates its sector boundaries automatically. Normally a track ends where the next one starts, or at the lead-out for the final track. On CD-Extra discs, the trailing audio/data session gap is excluded from the last audio track.

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;

// Track numbers come from the disc; do not assume the first audio track is #1.
let track = toc
    .tracks
    .iter()
    .find(|track| track.is_audio)
    .ok_or("no audio tracks found")?;
let data = reader.read_track(&toc, track.number)?;
```

`read_track` is a blocking call that buffers the complete track, so it can take some time and use hundreds of megabytes of memory. The streaming API instead returns sector-aligned chunks as they are read, keeping memory usage low and allowing progress reporting or playback before the complete track is available.

Streaming is still synchronous: each `next_chunk` call waits for the drive. This is often suitable for a CLI, where the loop can run on the main thread and report progress. A GUI should run the loop on a worker thread so drive reads do not block its event loop.

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;
let track = toc
    .tracks
    .iter()
    .find(|track| track.is_audio)
    .ok_or("no audio tracks found")?;

let mut stream = reader.open_track_stream(&toc, track.number)?;
while let Some(chunk) = stream.next_chunk()? {
    // Process one sector-aligned PCM chunk.
}
```

## Audio track format

Audio track data is raw [PCM](https://en.wikipedia.org/wiki/Pulse-code_modulation), the same uncompressed sample representation used by PCM WAV files. Audio CDs use signed 16-bit little-endian stereo PCM sampled at 44,100 Hz:

```text
44,100 sample frames * 2 channels * 2 bytes = 176,400 bytes/second
```

Each audio sector contains exactly 2,352 bytes (176,400 / 75 = 2,352), giving 75 sectors per second. A typical 3-minute track is about 31.8 MB (30.3 MiB). A 74-minute disc contains about 783 MB (747 MiB) of raw PCM; common 80-minute media contains about 847 MB (808 MiB).

`create_wav` prepends a standard 44-byte RIFF/WAVE header. It does not validate or convert the audio, but PCM returned by `CdReader::read_track` already has the required format:

```rust
use cd_da_reader::{CdReader, create_wav};

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;
let track = toc
    .tracks
    .iter()
    .find(|track| track.is_audio)
    .ok_or("no audio tracks found")?;
let data = reader.read_track(&toc, track.number)?;
let wav = create_wav(data);
let output = format!("track{:02}.wav", track.number);
std::fs::write(output, wav)?;
```

The returned vector contains a complete WAV file that can be written directly to disk.

## Read options

`CdReader::read_track` and `CdReader::open_track_stream` use the `ReadOptions` defaults: CD-DA audio sectors, the default retry policy, and no read-speed change. These settings are sufficient for most audio reads.

For more control, start with `ReadOptions::default()` and pass the configured options to `CdReader::read_track_with_options` or `CdReader::open_track_stream_with_options`. The configurable options are:

- **Sector format:** `SectorReadFormat` controls the type and layout of sectors returned by the drive. `SectorReadFormat::Audio` is the default. For a data track, `CdReader::detect_track_format` can select an appropriate default format to pass to `ReadOptions::with_format`.
- **Retry policy:** `RetryConfig` controls the number of attempts, retry delays, and adaptive reduction of the number of sectors requested after a failed read. Its defaults are suitable for most drives.
- **Read speed:** `ReadSpeed` requests an automatic or custom drive speed. The default, `ReadSpeed::Unchanged`, issues no speed-change request. Requested speeds are not guaranteed, and the crate does not restore the previous drive setting afterward. Speed behavior depends on the OS and drive firmware.

```rust
use cd_da_reader::{CdReader, ReadOptions, ReadSpeed, RetryConfig, SectorReadFormat};

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;
let track = toc
    .tracks
    .iter()
    .find(|track| track.is_audio)
    .ok_or("no audio tracks found")?;
let options = ReadOptions::default()
    .with_format(SectorReadFormat::Audio)
    .with_retry(RetryConfig::default().with_max_attempts(6))
    .with_read_speed(ReadSpeed::CustomMultiplier(4));
let data = reader.read_track_with_options(&toc, track.number, &options)?;
```

## Reading data tracks

Blocking and streaming reads use the same `ReadOptions`, so reading a data track requires selecting a matching `SectorReadFormat`. Call `CdReader::detect_track_format` explicitly to choose an appropriate default:

```rust
use cd_da_reader::{CdReader, ReadOptions, SectorReadFormat};

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;

// A data track is any track for which `is_audio` is false.
let data_track = toc
    .tracks
    .iter()
    .find(|track| !track.is_audio)
    .ok_or("no data track on this disc")?;

let format = reader.detect_track_format(data_track)?;
let options = ReadOptions::default().with_format(format);
let data = reader.read_track_with_options(&toc, data_track.number, &options)?;

match format {
    // A typical Mode 1 ISO 9660 track can be written as a mountable image.
    SectorReadFormat::Mode1Cooked => std::fs::write("disc.iso", &data)?,
    // Mode 2 remains raw and must be interpreted sector by sector.
    SectorReadFormat::Mode2Raw => std::fs::write("disc.mode2.bin", &data)?,
    other => return Err(format!("unexpected data-track format: {other:?}").into()),
}
```

`Mode1Cooked` returns the 2,048-byte user-data field from each sector, which is directly usable when the track contains a typical ISO 9660 filesystem. `Mode1Raw` returns complete 2,352-byte Mode 1 sectors. Mode 2 is detected as `Mode2Raw`; because Form 1 and Form 2 are per-sector properties, callers must inspect each sector's XA subheader and extract the appropriate payload themselves.

See `examples/save_data_track.rs` for a complete detect, stream, save, and platform-specific mounting workflow.

## Reading from a file image

Everything above a raw sector read is hardware-independent, so tracks can also come from an image, decoded container, in-memory disc, or another custom source. Implement `AudioSectorReader`, build a `Toc` from the source's track metadata, and ensure both use the same LBA address space. The reader must return CD-DA PCM as exactly 2,352 bytes per sector: signed 16-bit little-endian stereo at 44,100 Hz.

```rust
use cd_da_reader::{AudioSectorReader, create_wav, read_track};

impl AudioSectorReader for MyImage {
    type Error = std::io::Error;

    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
        // Return exactly count * 2,352 bytes of CD-DA PCM.
        todo!()
    }
}

// Build `toc` from the source's metadata and use its reported track number.
let track = toc
    .tracks
    .iter()
    .find(|track| track.is_audio)
    .ok_or("no audio tracks found")?;
let pcm = read_track(&image, &toc, track.number)?;
let wav = create_wav(pcm);
```

`CdReader` itself implements `AudioSectorReader`, so drive-backed and file-backed code share the generic `read_track` path.

Two examples cover this, both dependency-free:

- `examples/file_backend.rs` — the smallest possible backing (whole-disc PCM in memory), to show the shape of the trait.
- `examples/bin_cue_backend.rs` — a real container: it parses a `.cue` sheet into a `Toc` and serves sectors out of the `.bin` with positioned reads. Point it at an image with `cargo run --example bin_cue_backend -- /path/to/disc.cue`, or run it bare and it synthesizes a small mixed-mode image to work against.

One caveat worth knowing before writing a backing: `read_track` defaults to `TrackBounds::SessionGap`, which subtracts the CD-Extra inter-session gap from the last audio track before a trailing data session. That is correct for a physical disc or an image whose TOC preserves the disc's real LBAs. For a source whose tracks are addressed back-to-back with that gap stripped out—such as a single-`FILE` BIN/CUE or a `chdman extractcd` extract—the subtraction would remove 11,400 sectors (152 seconds) of real audio. Use `TrackBounds::Gapless` with `read_track_with_bounds` or `open_track_stream_with_bounds` for those sources.

## What about metadata?

Audio CDs carry almost no semantic metadata. [CD-TEXT](https://en.wikipedia.org/wiki/CD-Text) exists, but it is unreliable and is not provided by this crate.

The practical approach is to calculate a Disc ID from the full TOC and look it up through a service such as [MusicBrainz](https://musicbrainz.org/). The `Toc` exposes the track addresses and lead-out required by the [MusicBrainz disc ID algorithm](https://musicbrainz.org/doc/Disc_ID_Calculation). You can see an example calculation [here](https://github.com/Bloomca/audio-cd-ripper/blob/main/src/music_brainz/calculate_id.rs).
