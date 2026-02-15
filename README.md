## Rust CD-DA reader

[![Crates.io](https://img.shields.io/crates/v/cd-da-reader.svg)](https://crates.io/crates/cd-da-reader)
[![CI](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml/badge.svg?branch=main)](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml)

This is a library to read audio CDs. This is intended to be a fairly low-level library, it intends to read TOC and allow to read raw PCM tracks data (there is a [simple helper](https://docs.rs/cd-da-reader/0.1.0/cd_da_reader/struct.CdReader.html#method.create_wav) to prepend RIFF header to convert raw data to a wav file), but not to provide any encoders to MP3, Vorbis, FLAC, etc -- if you need that, you'd need to compose this library with some others.

It works on Windows, macOS and Linux, although each platform has slightly different behaviour regarding the handle exclusivity. Specifically, on macOS, it will not work if you use the audio CD somewhere -- the library will attempt to unmount it, claim exclusive access and only after read the data from it. After it is done, it will remount the CD back so other apps can use, which will cause the OS to treat as if you just inserted the CD.

For example, if you want to read TOC and save the first track as a WAV file, you can do the following:

```rust
let reader = CDReader::open_default()?;
let toc = reader.read_toc()?;

let first_audio_track = toc
        .tracks
        .iter()
        .find(|track| track.is_audio)
        .ok_or_else(|| std::io::Error::other("no audio tracks in TOC"))?;

let data = reader.read_track(&toc, last_audio_track.number)?;
let wav_track = CdReader::create_wav(data);
std::fs::write("myfile.wav", wav_track)?;
```

You can open a specific drive, but often the machine will have only 1 valid audio CD, so the default drive method should work in most scenarios. Reading track data is a pretty slow operation due to size and CD reading speeds, so there is a streaming API available if you want to interact with chunks of data directly.