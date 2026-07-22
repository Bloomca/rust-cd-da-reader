//! Read a track from a file/image backing instead of a physical drive.
//!
//! `cd-da-reader` doesn't bake in any image format (CHD, BIN/CUE, ...). Instead
//! you implement [`AudioSectorReader`] for your backing — supplying raw CD-DA
//! sectors (2352 bytes/sector, 16-bit signed little-endian stereo) — and reuse
//! the crate's TOC/track machinery via [`read_track`] and [`create_wav`].
//!
//! This example uses a tiny in-memory backing so it stays dependency-free. A
//! real backing (e.g. CHD via `libchdman-rs`) would decode sectors in
//! `read_audio_sectors` and build its TOC from the image's track metadata.
//!
//! Run with: `cargo run --example file_backend`
mod common;

use cd_da_reader::{
    AudioSectorReader, Toc, Track, create_wav, lba_to_msf, open_track_stream, read_track,
};

/// A backing that holds whole-disc PCM in memory and slices it per sector.
struct InMemoryDisc {
    /// Whole-disc PCM, laid out as 2352 bytes per sector.
    pcm: Vec<u8>,
}

impl AudioSectorReader for InMemoryDisc {
    type Error = std::io::Error;

    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
        let start = start_lba as usize * 2352;
        let end = start + count as usize * 2352;
        self.pcm.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "read past end of disc")
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = common::fresh_output_dir("file_backend")?;

    // A real backing derives this TOC from the image's own track metadata.
    // Here we fabricate a 2-track disc: 2 seconds + 3 seconds of audio.
    let track1_sectors = 75 * 2;
    let track2_sectors = 75 * 3;
    let total_sectors = track1_sectors + track2_sectors;

    let toc = Toc {
        first_track: 1,
        last_track: 2,
        tracks: vec![
            Track {
                number: 1,
                start_lba: 0,
                start_msf: lba_to_msf(0),
                is_audio: true,
            },
            Track {
                number: 2,
                start_lba: track1_sectors,
                start_msf: lba_to_msf(track1_sectors),
                is_audio: true,
            },
        ],
        leadout_lba: total_sectors,
    };

    // Silence, just for the demo — a real backing decodes actual audio here.
    let disc = InMemoryDisc {
        pcm: vec![0u8; total_sectors as usize * 2352],
    };

    for track in &toc.tracks {
        let pcm = read_track(&disc, &toc, track.number)?;
        println!(
            "track {}: {} bytes ({} sectors)",
            track.number,
            pcm.len(),
            pcm.len() / 2352
        );

        let wav = create_wav(pcm);
        let output_path = output_dir.join(format!("track{:02}.wav", track.number));
        std::fs::write(&output_path, wav)?;
        println!("  wrote {}", output_path.display());
    }

    // The same backing can be streamed instead of buffered: pull sector-aligned
    // chunks so a player never holds a whole track in memory at once. (A backing
    // whose tracks are addressed contiguously — a gap-stripped extract — would
    // open with `TrackBounds::Gapless`, or supply its own bounds via
    // `open_track_stream_at`; this demo TOC has no trailing data track, so plain
    // `open_track_stream` is equivalent.)
    let mut stream = open_track_stream(&disc, &toc, 1)?;
    let (mut chunks, mut bytes) = (0u32, 0usize);
    while let Some(chunk) = stream.next_chunk()? {
        chunks += 1;
        bytes += chunk.len();
    }
    println!(
        "streamed track 1: {bytes} bytes in {chunks} chunks ({:.1}s of audio)",
        stream.total_seconds()
    );

    Ok(())
}
