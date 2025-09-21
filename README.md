## Rust CD-DA reader

[![Crates.io](https://img.shields.io/crates/v/cd-da-reader.svg)](https://crates.io/crates/cd-da-reader)
[![CI](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml/badge.svg?branch=main)](https://github.com/Bloomca/rust-cd-da-reader/actions/workflows/pull-request-workflow.yaml)

This is a library to read audio CDs. This is intended to be a fairly low-level library, it intends to read TOC and allow to read raw PCM tracks data (there is a [simple helper](https://docs.rs/cd-da-reader/0.1.0/cd_da_reader/struct.CdReader.html#method.create_wav) to prepend RIFF header to convert raw data to a wav file), but not to provide any encoders to MP3, Vorbis, FLAC, etc -- if you need that, you'd need to compose this library with some others.

It works on Windows, macOS and Linux, although each platform has slightly different behaviour regarding the handle exclusivity. Specifically, on macOS, it will not work if you use the audio CD somewhere -- the library will attempt to unmount it, claim exclusive access and only after read the data from it. After it is done, it will remount the CD back so other apps can use, which will cause the OS to treat as if you just inserted the CD.

There is an example to read TOC and save a track ([ref](./examples/read_track.rs)); the example is cross-platform, but you'll likely need to adjust the drive letter. On Windows, simply look in your File Explorer; on macOS, execute `diskutil list` and find the drive with `Audio CD` name; on Linux, call `cat /proc/sys/dev/cdrom/info`.