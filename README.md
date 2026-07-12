## Rust CD-DA reader

[![Crates.io](https://img.shields.io/crates/v/cd-da-reader.svg)](https://crates.io/crates/cd-da-reader)
[![CI](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml/badge.svg?branch=main)](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml)

This is a simple library to read audio CDs. At the core it was written to enable CD ripping, but you can also implement a live audio CD player with its help. It is cross-platform and tested on Windows, macOS and Linux and abstracts both access to the CD drive and reading the actual data from it. All operations happen in this order on each platform:

1. Get a CD drive's handle
2. Read ToC (table of contents) of the audio CD
3. Read track data using ranges from ToC

Let's go through each concept in order.

## CD access

First thing, we'll need to get a hold of the CD drive. You can see the drive's letter on Windows in File Explorer (although the actual handle will be something like `"\\.\E:"`), with `cat /proc/sys/dev/cdrom/info` on Linux and with `diskutil list` on macOS.

This is a bit brittle, so this library provides a few helper methods to find a correct CD drive. By far the most straightforward approach is to simply open the "default" drive:

```rust
use cd_da_reader::{CdReader};

let reader = CdReader::open_default()?;
```

This code will scan the CD drives and will open the first one with an audio CD in it, and _usually_ this is what you want. If you want to provide a choice, there is an additional function to list all drives:

```rust
use cd_da_reader::{CdReader};

let drives = CdReader::list_drives()?;
```

This gives you a vector of drives. Each entry has a `has_audio_cd` field and can
be opened directly:

```rust
use cd_da_reader::CdReader;

let drives = CdReader::list_drives()?;
let selected = drives.first().ok_or("no optical drives found")?;
let reader = CdReader::open(selected)?;
```

If you already know the platform-specific device path, use `open_path`:

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_path("disk14")?;
```

## Reading ToC

Each audio CD provides internal Table of Contents, which is an internal map of all the available tracks with the block addresses. The only semantic metadata we get from it is the number of tracks, but it is crucial to read it so that we can issues commands to read actual tracks data.

```rust
use cd_da_reader::{CdReader};

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;
```

This will give us a struct like:

```
{
    first_track: 1,
    last_track: 11,
    tracks: [{
        number: 1,
        start_lba: 0,
        start_msf: (0, 2, 0),
        is_audio: true,
    }, {
        number: 1,
        start_lba: 14675,
        start_msf: (3, 15, 50),
        is_audio: true,
    }, ...],
    leadout_lba: 221786
}
```

**LBA (Logical Block Address)** is a simple sequential sector index. LBA 0 is the first readable sector after the 2-second lead-in pre-gap at the start of every disc. It is the most convenient format for issuing read commands and used internally to read data blocks.

**MSF (Minutes:Seconds:Frames)** is a time-based address inherited from the physical disc layout. A "frame" here is one CD sector, and the spec defines 75 frames per second. MSF includes a fixed 2-second (150-frame) offset for the lead-in area, so `MSF (0, 2, 0)` corresponds to LBA 0 — the very start of track data.

The two are fully interchangeable: `LBA + 150 = total frames from disc start`, from which minutes, seconds, and frames are derived by dividing by 75 and 60. You will typically only need LBA values for reading track data, while MSF is required for services like MusicBrainz disc ID calculation.

## Reading tracks

Finally, after we got ToC, we can read tracks. The usual boundaries for the track are the starting LBA and the starting LBA for the next track (or leadout LBA value for the last track). For CD-Extra discs where the last audio track is followed only by data tracks, the library subtracts the standard 11,400-sector audio/data session gap from the first data track start. This library abstracts these things and simply reads provided track numbers. To read a track, all you need to do is call:

```rust
use cd_da_reader::{CdReader};

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;
// we assume that track #1 exists for simplicity
let data = reader.read_track(&toc, 1)?;
```

This is a blocking call and takes a lot of time (depends on the track length and CD/drive quality due to retries). If you want to do something with the data as it comes, use streaming API:

```rust
use cd_da_reader::CdReader;

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;

let mut stream = reader.open_track_stream(&toc, 1)?;
while let Some(chunk) = stream.next_chunk()? {
    // do something with the chunk directly
}
```

## Track format

The data you receive by reading tracks is [PCM](https://en.wikipedia.org/wiki/Pulse-code_modulation), the same raw format used by WAV files. Audio CDs use 16-bit stereo PCM sampled at 44,100 Hz, so each second of audio is:

```
44,100 samples * 2 channels * 2 bytes = 176,400 bytes/second
```

Each CD sector holds exactly 2,352 bytes of audio payload (176,400 / 75 = 2,352), that's why there are 75 sectors per second. A typical 3-minute track is about 31 MB of raw PCM, and a full 74-minute CD holds ~650 MB.

Converting PCM data to a playable WAV file only requires prepending a 44-byte RIFF header. In fact, there is a helper for that in this library:

```rust
use cd_da_reader::{CdReader};

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;
// we assume that track #1 exists for simplicity
let data = reader.read_track(&toc, 1)?;
let wav = CdReader::create_wav(data);
std::fs::write("myfile.wav", wav)?;
```

This code will read the first track from the CD file and save it as a WAVE file, which will be playable by any music player.

## Reading data tracks

Blocking reads and streaming reads share the same options struct, so switching from audio to data is just a matter of the format you pass. Every track's format can be auto-detected:

```rust
use cd_da_reader::{CdReader, ReadOptions, SectorReadFormat};

let reader = CdReader::open_default()?;
let toc = reader.read_toc()?;

// A "data track" is simply `!is_audio` — there is no dedicated helper.
let data_track = toc.tracks.iter().find(|t| !t.is_audio)
    .ok_or("no data track on this disc")?;

// Mode 1 data tracks detect as Mode1Cooked (2048 B user data per sector),
// which is exactly the ISO 9660 image — write it out and mount it.
let format = reader.detect_track_format(data_track)?;
let options = ReadOptions::default().with_format(format);
let image = reader.read_track_with_options(&toc, data_track.number, &options)?;
std::fs::write("disc.iso", &image)?;
```

Mode 1 is fully handled (`Mode1Cooked` for the ready-to-mount user data, `Mode1Raw` for the complete 2352-byte sector). Mode 2 is *detected* (`Mode2Raw`) but its per-sector XA payload extraction is left to the consumer. The full workflow — detect, save, and platform-specific mount commands — is in `examples/save_data_track.rs`, and the detailed guide is [docs/consuming-cd-da-reader.md](docs/consuming-cd-da-reader.md).

## Reading from a file image

Everything above a raw sector read is hardware-independent, so you can read tracks from an image (CHD, BIN/CUE, an in-memory buffer, ...) instead of a drive. Implement `AudioSectorReader` for your backing — it must return raw sectors in the exact CD-DA format the physical reader produces: 2352 bytes/sector, 16-bit signed little-endian, stereo — and reuse the crate's TOC/track machinery, with no image-format dependencies pulled into this crate:

```rust
use cd_da_reader::{AudioSectorReader, create_wav, read_track};

impl AudioSectorReader for MyImage {
    type Error = std::io::Error;
    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
        // return exactly count * 2352 bytes of little-endian PCM
        todo!()
    }
}

let pcm = read_track(&image, &toc, 1)?;   // build `toc` from the image's metadata
let wav = create_wav(pcm);                // free fn; also CdReader::create_wav
```

`CdReader` itself implements `AudioSectorReader`, so drive-backed and file-backed code share the generic `read_track` path. See `examples/file_backend.rs` for a complete, dependency-free example.

## What about metadata?

You might have asked why do we expose LBA/MSF values if the track reading is abstracted behind specific track numbers. The reason for that is metadata. Even though there is a command [CD-TEXT](https://en.wikipedia.org/wiki/CD-Text) for storing data directly, it is not exposed in this library due to it being extremely unreliable.

Instead, you can calculate a Disc ID for a service like [MusicBrainz](https://musicbrainz.org/), which requires full ToC for it: [ref](https://musicbrainz.org/doc/Disc_ID_Calculation). You can see an example of how to calculate the ID [here](https://github.com/Bloomca/audio-cd-ripper/blob/main/src/music_brainz/calculate_id.rs).
