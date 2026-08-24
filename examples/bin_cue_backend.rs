//! Read audio tracks out of a BIN/CUE disc image, with no image-format crate.
//!
//! BIN/CUE is the simplest real backing there is: the `.bin` holds the disc's
//! sectors end to end, and the `.cue` says where each track starts. That makes
//! it a good worked example of the whole [`AudioSectorReader`] contract —
//! parse the container's own metadata into a [`Toc`], serve raw 2352-byte
//! sectors, and let the crate do the track math and WAV wrapping.
//!
//! Three things here are worth copying into a real backing:
//!
//! 1. **`&self` reads via positioned I/O.** `read_audio_sectors` takes `&self`,
//!    so the backing uses `read_at`/`seek_read` (an offset per call) rather than
//!    `seek` + `read`, which would need `&mut self`.
//! 2. **`TrackBounds::Gapless`.** A single-`FILE` cue is contiguous by
//!    construction — every `INDEX 01` is an offset into the same `.bin`, so
//!    track N+1 begins exactly where track N ends. There is no inter-session gap
//!    in the addressing, so the CD-Extra trailing-gap rule must not be applied.
//!    See the note this example prints for a mixed-mode disc.
//! 3. **`type Error = io::Error`.** Any `std::error::Error + Send + Sync` works;
//!    the crate reports it as [`CdReaderError::Backend`] with the original error
//!    kept as its `source()`.
//!
//! Run against a real image:
//!
//! ```text
//! cargo run --example bin_cue_backend -- /path/to/disc.cue
//! ```
//!
//! Run with no arguments and it writes a small mixed-mode BIN/CUE into the
//! output directory first, so the example works without an image on hand:
//!
//! ```text
//! cargo run --example bin_cue_backend
//! ```
mod common;

use std::error::Error;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use cd_da_reader::{
    AudioSectorReader, Toc, Track, TrackBounds, create_wav, lba_to_msf,
    open_track_stream_with_bounds, read_track_with_bounds,
};

/// Every sector in a raw disc image is 2352 bytes, audio or data alike.
const SECTOR_SIZE: usize = 2352;
const SECTORS_PER_SECOND: u32 = 75;

/// A single-`FILE` BIN/CUE image: the `.bin` addressed by sector.
///
/// Sector *n* of the image lives at byte `n * 2352`, and because the cue's
/// offsets are offsets into this same file, that sector index *is* the LBA the
/// [`Toc`] carries — no translation needed.
struct BinImage {
    bin: File,
}

impl AudioSectorReader for BinImage {
    type Error = io::Error;

    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
        let mut buffer = vec![0u8; count as usize * SECTOR_SIZE];
        read_exact_at(
            &self.bin,
            &mut buffer,
            start_lba as u64 * SECTOR_SIZE as u64,
        )?;
        Ok(buffer)
    }
}

/// Positioned read: fill `buffer` from `offset` without moving a shared cursor,
/// which is what lets `read_audio_sectors` take `&self`.
fn read_exact_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(buffer, offset)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;

        let mut filled = 0;
        while filled < buffer.len() {
            match file.seek_read(&mut buffer[filled..], offset + filled as u64) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "read past the end of the .bin",
                    ));
                }
                Ok(read) => filled += read,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let output_dir = common::fresh_output_dir("bin_cue_backend")?;

    let cue_path = match std::env::args().nth(1) {
        Some(path) => PathBuf::from(path),
        None => {
            println!("No .cue argument given — writing a demo image to work against.\n");
            write_demo_image(&output_dir)?
        }
    };

    let (bin_path, toc) = parse_cue(&cue_path)?;
    println!("Cue:   {}", cue_path.display());
    println!("Bin:   {}", bin_path.display());
    println!(
        "Toc:   {} tracks, leadout at LBA {}\n",
        toc.tracks.len(),
        toc.leadout_lba
    );

    let image = BinImage {
        bin: File::open(&bin_path)?,
    };

    // A single-FILE cue is a contiguous run of sectors, so the CD-Extra
    // inter-session gap is not part of the addressing. See `explain_bounds`.
    let bounds = TrackBounds::Gapless;
    explain_bounds(&image, &toc)?;

    for track in toc.tracks.iter().filter(|track| track.is_audio) {
        let pcm = read_track_with_bounds(&image, &toc, track.number, bounds)?;
        let wav_path = output_dir.join(format!("track{:02}.wav", track.number));
        std::fs::write(&wav_path, create_wav(pcm))?;

        println!("track {:02}: wrote {}", track.number, wav_path.display());
    }

    // The same backing streams instead of buffering — for a 74-minute image
    // that is the difference between one chunk and ~650 MB resident.
    if let Some(first_audio) = toc.tracks.iter().find(|track| track.is_audio) {
        let mut stream = open_track_stream_with_bounds(&image, &toc, first_audio.number, bounds)?;
        let (mut chunks, mut bytes) = (0u32, 0usize);
        while let Some(chunk) = stream.next_chunk()? {
            chunks += 1;
            bytes += chunk.len();
        }
        println!(
            "\nstreamed track {:02}: {bytes} bytes in {chunks} chunks ({:.1}s)",
            first_audio.number,
            stream.total_seconds()
        );
    }

    Ok(())
}

/// Show what the two [`TrackBounds`] policies do on this disc.
///
/// They differ on exactly one track — the last audio track before a trailing
/// data session — so on a plain audio disc this prints nothing.
fn explain_bounds(image: &BinImage, toc: &Toc) -> Result<(), Box<dyn Error>> {
    let Some(track) = last_audio_before_data(toc) else {
        return Ok(());
    };

    let gapless =
        open_track_stream_with_bounds(image, toc, track, TrackBounds::Gapless)?.total_sectors();
    let physical = match open_track_stream_with_bounds(image, toc, track, TrackBounds::SessionGap) {
        Ok(stream) => format!("{} sectors", stream.total_sectors()),
        // The gap is larger than the track, so subtracting it underflows.
        Err(e) => format!("fails ({e})"),
    };

    println!(
        "Track {track:02} is the last audio track before a data track, the one track \
         the two bounds policies disagree on:\n  \
         Gapless (used here): {gapless} sectors\n  \
         SessionGap:          {physical}\n"
    );
    Ok(())
}

fn last_audio_before_data(toc: &Toc) -> Option<u8> {
    let last_audio = toc.tracks.iter().rposition(|track| track.is_audio)?;
    let has_trailing_data = last_audio + 1 < toc.tracks.len();

    has_trailing_data.then(|| toc.tracks[last_audio].number)
}

/// Parse a single-`FILE` cue sheet into the `.bin` path and a [`Toc`].
///
/// Only the four lines that matter for reading sectors are interpreted — `FILE`,
/// `TRACK`, `INDEX 01`, and the track mode — which is all a cue needs to carry
/// for this purpose. `REM`, `TITLE`, `PERFORMER`, and friends are metadata and
/// are skipped.
///
/// Two cue features are deliberately not modelled, because neither changes a
/// byte offset in the `.bin`:
///
/// - `INDEX 00` marks a pregap that *is* stored in the file. Tracks start at
///   `INDEX 01`, so those sectors fall at the end of the preceding track — the
///   usual "gap appended to the previous track" layout.
/// - `PREGAP` declares silence that is *not* stored in the file. It shifts a
///   disc's addressing but not this file's, and we address by file offset.
fn parse_cue(cue_path: &Path) -> Result<(PathBuf, Toc), Box<dyn Error>> {
    let text = std::fs::read_to_string(cue_path)?;
    let cue_dir = cue_path.parent().unwrap_or(Path::new("."));

    let mut bin_path: Option<PathBuf> = None;
    let mut pending: Option<(u8, bool)> = None;
    let mut tracks: Vec<Track> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        let mut fields = line.split_whitespace();
        let Some(keyword) = fields.next() else {
            continue;
        };

        match keyword.to_ascii_uppercase().as_str() {
            "FILE" => {
                if bin_path.is_some() {
                    return Err(
                        "multi-FILE cue sheets (one file per track) are not handled by \
                                this example; it assumes a single .bin addressed by sector"
                            .into(),
                    );
                }
                bin_path = Some(cue_dir.join(quoted_or_first_field(line)?));
            }

            "TRACK" => {
                // e.g. `TRACK 03 MODE1/2352`
                let number: u8 = fields.next().ok_or("TRACK line has no number")?.parse()?;
                let mode = fields.next().ok_or("TRACK line has no mode")?;

                // The whole image is addressed as a uniform grid of 2352-byte
                // sectors, so a cooked data track (MODE1/2048) would desync
                // every offset after it. Refuse rather than read garbage.
                let sector_size = mode_sector_size(mode);
                if sector_size != Some(SECTOR_SIZE) {
                    return Err(format!(
                        "track {number} is `{mode}`, which is not stored as 2352-byte sectors; \
                         this example needs a fully raw image"
                    )
                    .into());
                }

                pending = Some((number, mode.eq_ignore_ascii_case("AUDIO")));
            }

            // `INDEX 01 MM:SS:FF` is where the track proper begins.
            "INDEX" => {
                let index = fields.next().ok_or("INDEX line has no number")?;
                let msf = fields.next().ok_or("INDEX line has no timestamp")?;
                if index != "01" {
                    continue;
                }

                let (number, is_audio) = pending
                    .take()
                    .ok_or("INDEX 01 appeared before any TRACK line")?;
                let start_lba = msf_to_frames(msf)?;

                tracks.push(Track {
                    number,
                    start_lba,
                    start_msf: lba_to_msf(start_lba),
                    is_audio,
                });
            }

            _ => {}
        }
    }

    let bin_path = bin_path.ok_or("cue sheet has no FILE line")?;
    if tracks.is_empty() {
        return Err("cue sheet declares no tracks".into());
    }
    tracks.sort_by_key(|track| track.start_lba);

    // The cue has no leadout; the image's own length is the end of the disc.
    let bin_bytes = std::fs::metadata(&bin_path)
        .map_err(|e| format!("cannot open {}: {e}", bin_path.display()))?
        .len();
    let leadout_lba = (bin_bytes / SECTOR_SIZE as u64) as u32;

    let toc = Toc {
        first_track: tracks.first().map_or(1, |track| track.number),
        last_track: tracks.last().map_or(1, |track| track.number),
        tracks,
        leadout_lba,
    };

    Ok((bin_path, toc))
}

/// Bytes per sector for a cue track mode, or `None` if the mode is unknown.
/// `AUDIO` is always raw; the rest carry their size after a slash
/// (`MODE1/2048`, `MODE1/2352`, `MODE2/2352`, ...).
fn mode_sector_size(mode: &str) -> Option<usize> {
    if mode.eq_ignore_ascii_case("AUDIO") {
        return Some(SECTOR_SIZE);
    }

    mode.split_once('/').and_then(|(_, size)| size.parse().ok())
}

/// `MM:SS:FF` to a sector count. These are offsets into the `.bin`, so unlike a
/// disc MSF address there is no 150-frame lead-in to subtract.
fn msf_to_frames(msf: &str) -> Result<u32, Box<dyn Error>> {
    let parts: Vec<&str> = msf.split(':').collect();
    let [minutes, seconds, frames] = parts[..] else {
        return Err(format!("expected a MM:SS:FF timestamp, got `{msf}`").into());
    };

    let minutes: u32 = minutes.parse()?;
    let seconds: u32 = seconds.parse()?;
    let frames: u32 = frames.parse()?;

    Ok(((minutes * 60) + seconds) * SECTORS_PER_SECOND + frames)
}

/// The filename from a `FILE "name with spaces.bin" BINARY` line, falling back
/// to the bare second field when it is unquoted.
fn quoted_or_first_field(line: &str) -> Result<String, Box<dyn Error>> {
    if let Some((_, rest)) = line.split_once('"') {
        return rest
            .rsplit_once('"')
            .map(|(name, _)| name.to_string())
            .ok_or_else(|| "FILE line has an unterminated quote".into());
    }

    line.split_whitespace()
        .nth(1)
        .map(str::to_string)
        .ok_or_else(|| "FILE line has no filename".into())
}

/// Write a small mixed-mode BIN/CUE (two audio tracks then a data track) so the
/// example runs without an image on hand, and returns the `.cue` path.
///
/// The trailing data track is the point: it makes this disc one where the two
/// [`TrackBounds`] policies actually disagree.
fn write_demo_image(output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let layout = [
        (1u8, "AUDIO", 2 * SECTORS_PER_SECOND, Some(440.0)),
        (2, "AUDIO", 2 * SECTORS_PER_SECOND, Some(523.25)),
        (3, "MODE1/2352", SECTORS_PER_SECOND, None),
    ];

    let mut bin = Vec::new();
    let mut cue = String::from("FILE \"demo.bin\" BINARY\n");
    let mut start_lba = 0;

    for (number, mode, sectors, tone_hz) in layout {
        cue += &format!(
            "  TRACK {number:02} {mode}\n    INDEX 01 {}\n",
            frames_to_msf(start_lba)
        );

        match tone_hz {
            Some(hz) => bin.extend_from_slice(&tone(sectors, hz)),
            // Not a real ISO 9660 filesystem — just something that is clearly
            // not audio, since the example never reads data tracks.
            None => bin.resize(bin.len() + sectors as usize * SECTOR_SIZE, 0xAA),
        }
        start_lba += sectors;
    }

    let cue_path = output_dir.join("demo.cue");
    std::fs::write(output_dir.join("demo.bin"), &bin)?;
    std::fs::write(&cue_path, cue)?;

    Ok(cue_path)
}

/// `sectors` worth of a stereo sine at `hz`, as CD-DA PCM.
fn tone(sectors: u32, hz: f32) -> Vec<u8> {
    // 2352 bytes per sector / 4 bytes per stereo frame.
    let frames = sectors as usize * 588;
    let mut pcm = Vec::with_capacity(frames * 4);

    for frame in 0..frames {
        let t = frame as f32 / 44_100.0;
        let sample = ((t * hz * std::f32::consts::TAU).sin() * 8_000.0) as i16;
        pcm.extend_from_slice(&sample.to_le_bytes()); // left
        pcm.extend_from_slice(&sample.to_le_bytes()); // right
    }

    pcm
}

/// Sector count to the `MM:SS:FF` a cue sheet expects (file-relative, no lead-in).
fn frames_to_msf(frames: u32) -> String {
    let seconds = frames / SECTORS_PER_SECOND;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 60,
        seconds % 60,
        frames % SECTORS_PER_SECOND
    )
}
