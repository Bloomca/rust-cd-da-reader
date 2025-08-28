## Rust CD-DA reader

This is a library to read audio CDs. This is intended to be a fairly low-level library, it intends to read TOC and allow to read raw PCM tracks data (with a simple RIFF header you can save it as a `.wav` file and play using any standard OS player), but not to provide any encoders to MP3, Vorbis, FLAC, etc -- if you need that, you'd need to compose this library with some others.

It is intended to work on Linux, macOS and Windows (I don't have BSD, so that one likely won't work). Currently it works on Windows on macOS, but I plan to add Linux support as well.

It is not published as a crate yet, but you can run it using `cargo run` on Windows and macOS; you'd need to adjust the CDROM drive first in `main.rs` file. On Windows, simply look in your File Explorer, and on macOS, execute `diskutil list` and find the drive with `Audio CD` name.