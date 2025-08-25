## Rust CD-DA reader

This is a library to read audio CDs. This is intended to be a fairly low-level library, it intends to read TOC and allow to read raw PCM tracks data (with a simple RIFF header you can save it as a `.wav` file and play using any standard OS player), but not to provide any encoders to MP3, Vorbis, FLAC, etc -- if you need that, you'd need to compose this library with some others.

It is intended to work on Linux, macOS and Windows (I don't have BSD, so that one likely won't work). Currently it works only on Windows, but it will work on other platforms in the future as well.

It is not published as a crate yet, but you can run it using `cargo run` on Windows (you'd need to adjust the CDROM drive first).