//! Pluggable audio-sector backings.
//!
//! [`CdReader`](crate::CdReader) reads CD-DA sectors from a physical drive over
//! SCSI/ioctl, but everything *above* the raw sector read — the
//! [`Track`](crate::Track)/[`Toc`](crate::Toc) types, the track-bounds math
//! (including the CD-Extra trailing-gap rule), and WAV wrapping — is
//! hardware-independent. [`AudioSectorReader`] exposes that seam so any backing
//! that can produce raw CD-DA sectors (a CHD image, a BIN/CUE dump, an in-memory
//! buffer, a network stream, ...) reuses the same machinery **without this crate
//! taking on any image-format dependencies**.
//!
//! The image format lives in the caller: implement [`AudioSectorReader`] for
//! your backing, build a [`Toc`](crate::Toc) from the image's own track metadata
//! (see [`lba_to_msf`](crate::lba_to_msf)), then read PCM in the exact same
//! little-endian, 2352-byte/sector format the physical reader produces — ready
//! for [`create_wav`](crate::create_wav). Read a whole track at once with
//! [`read_track`], or pull it incrementally with [`open_track_stream`] (the
//! file/image counterpart to [`TrackStream`](crate::TrackStream)).
//!
//! ## Physical vs. gapless track bounds
//!
//! Resolving a track's sector range from a [`Toc`] differs between a physical
//! disc and an extracted image on exactly one track: the last audio track before
//! a trailing data session on a CD-Extra disc. A physical disc has a real
//! inter-session gap there; an extracted CHD/BIN image is laid out gapless.
//! [`read_track`] / [`open_track_stream`] default to
//! [`TrackBounds::PhysicalDisc`]; image backings must pass
//! [`TrackBounds::GaplessImage`] (or supply explicit bounds via
//! [`open_track_stream_at`]) or that track loses ~2.5 min of audio.
//!
//! See `examples/file_backend.rs` for a complete, dependency-free example.

use std::cmp::min;

use crate::{CdReader, CdReaderError, ReadOptions, Toc, utils};

/// The physical drive is itself an [`AudioSectorReader`], so drive-backed and
/// file-backed code can share the generic [`read_track`] path. This uses the
/// default read options (audio sectors, default retry policy); for explicit
/// control, prefer the inherent [`CdReader::read_track_with_options`].
impl AudioSectorReader for CdReader {
    type Error = CdReaderError;

    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
        self.read_sector_range(start_lba, count, &ReadOptions::default())
    }
}

/// A source of raw CD-DA audio sectors.
///
/// Implement this for any backing that can yield audio in the crate's canonical
/// format: **2352 bytes per sector, 16-bit signed little-endian, stereo, 44100
/// Hz** — byte-for-byte identical to what
/// [`CdReader::read_track`](crate::CdReader::read_track) returns.
///
/// `start_lba` is an absolute Logical Block Address (a sector index; LBA 0 is
/// the first sector after the lead-in), matching
/// [`Track::start_lba`](crate::Track::start_lba). A successful read of `count`
/// sectors must return exactly `count * 2352` bytes.
///
/// The read takes `&self`, matching [`CdReader`]. A backing that needs a mutable
/// handle (an open `File`, a decoder) should use positioned reads
/// (`read_at`/`seek_read`) or interior mutability so shared-borrow reads stay
/// possible.
pub trait AudioSectorReader {
    /// Error type produced by this backing.
    type Error;

    /// Read `count` sectors starting at absolute `start_lba`, returning exactly
    /// `count * 2352` bytes of little-endian PCM.
    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error>;
}

/// How a track's sector range is resolved from a [`Toc`].
///
/// The two policies differ on exactly one track: the **last audio track before a
/// trailing data session** on a CD-Extra disc. Every other track resolves
/// identically.
///
/// - [`PhysicalDisc`](Self::PhysicalDisc) subtracts the inter-session gap
///   (matching [`CdReader::read_track`](crate::CdReader::read_track)) — correct
///   when reading a real disc, where that gap is present.
/// - [`GaplessImage`](Self::GaplessImage) does not: an extracted CHD/BIN image
///   lays tracks back-to-back, so a track spans from its `start_lba` to the next
///   track's start (or the leadout). Subtracting the gap there would drop ~2.5
///   min of real audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackBounds {
    /// Physical-disc geometry: apply the CD-Extra trailing-gap rule.
    PhysicalDisc,
    /// Gapless extracted-image geometry: no inter-session gap subtraction.
    GaplessImage,
}

impl TrackBounds {
    fn resolve(self, toc: &Toc, track_no: u8) -> std::io::Result<(u32, u32)> {
        match self {
            TrackBounds::PhysicalDisc => utils::get_track_bounds(toc, track_no),
            TrackBounds::GaplessImage => utils::get_gapless_track_bounds(toc, track_no),
        }
    }
}

/// Read raw PCM for one track from any [`AudioSectorReader`] backing, using
/// physical-disc track geometry ([`TrackBounds::PhysicalDisc`]).
///
/// This is the file/image counterpart to
/// [`CdReader::read_track`](crate::CdReader::read_track): it resolves the track's
/// sector range from `toc` (honouring the CD-Extra trailing-gap rule) and pulls
/// those sectors from `src`. The returned bytes are the same little-endian,
/// 2352-B/sector PCM, so [`create_wav`](crate::create_wav) wraps them into a
/// playable file unchanged.
///
/// A bad track request (not in the TOC, or invalid bounds) is
/// [`CdReaderError::Io`]; a failure inside the backing is
/// [`CdReaderError::Backend`], which preserves the backing's own error as the
/// boxed [`source`](std::error::Error::source).
///
/// For a **gapless** extracted CHD/BIN image, use [`read_track_with_bounds`]
/// with [`TrackBounds::GaplessImage`].
pub fn read_track<R>(src: &R, toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdReaderError>
where
    R: AudioSectorReader,
    R::Error: std::error::Error + Send + Sync + 'static,
{
    read_track_with_bounds(src, toc, track_no, TrackBounds::PhysicalDisc)
}

/// Read one track like [`read_track`], but with an explicit [`TrackBounds`]
/// geometry — pass [`TrackBounds::GaplessImage`] for extracted CHD/BIN images.
pub fn read_track_with_bounds<R>(
    src: &R,
    toc: &Toc,
    track_no: u8,
    bounds: TrackBounds,
) -> Result<Vec<u8>, CdReaderError>
where
    R: AudioSectorReader,
    R::Error: std::error::Error + Send + Sync + 'static,
{
    let (start_lba, sectors) = bounds.resolve(toc, track_no).map_err(CdReaderError::Io)?;
    src.read_audio_sectors(start_lba, sectors)
        .map_err(|e| CdReaderError::Backend(Box::new(e)))
}

/// Streaming reader over an [`AudioSectorReader`] backing — the file/image
/// counterpart to [`TrackStream`](crate::TrackStream).
///
/// Pulls a track's PCM in sector-aligned chunks with
/// [`next_chunk`](Self::next_chunk) instead of buffering the whole track, so a
/// player can start immediately and hold only one chunk at a time. Open one
/// with [`open_track_stream`] (TOC + physical geometry),
/// [`open_track_stream_with_bounds`] (TOC + explicit [`TrackBounds`]), or
/// [`open_track_stream_at`] (an explicit absolute sector range, for backings
/// that compute their own bounds).
pub struct AudioTrackStream<'a, R: AudioSectorReader> {
    src: &'a R,
    start_lba: u32,
    next_lba: u32,
    remaining_sectors: u32,
    total_sectors: u32,
    sectors_per_chunk: u32,
}

impl<'a, R: AudioSectorReader> AudioTrackStream<'a, R> {
    const DEFAULT_SECTORS_PER_CHUNK: u32 = 27;
    const SECTORS_PER_SECOND: f32 = 75.0;

    fn new(src: &'a R, start_lba: u32, sectors: u32) -> Self {
        Self {
            src,
            start_lba,
            next_lba: start_lba,
            remaining_sectors: sectors,
            total_sectors: sectors,
            sectors_per_chunk: Self::DEFAULT_SECTORS_PER_CHUNK,
        }
    }

    /// Set the target chunk size in sectors (default 27; a full chunk is
    /// `sectors_per_chunk * 2352` bytes). Zero is normalized to one.
    pub fn with_sectors_per_chunk(mut self, sectors: u32) -> Self {
        self.sectors_per_chunk = sectors.max(1);
        self
    }

    /// Read the next chunk of PCM, or `Ok(None)` at end-of-track.
    ///
    /// Each chunk is `sectors_per_chunk * 2352` bytes except possibly the last.
    /// A backing failure is [`CdReaderError::Backend`]; the position does not
    /// advance on error, so a retry re-reads the same chunk.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CdReaderError>
    where
        R::Error: std::error::Error + Send + Sync + 'static,
    {
        if self.remaining_sectors == 0 {
            return Ok(None);
        }

        let sectors = min(self.remaining_sectors, self.sectors_per_chunk);
        let chunk = self
            .src
            .read_audio_sectors(self.next_lba, sectors)
            .map_err(|e| CdReaderError::Backend(Box::new(e)))?;

        self.next_lba += sectors;
        self.remaining_sectors -= sectors;

        Ok(Some(chunk))
    }

    /// Total number of sectors in this track.
    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }

    /// Current position as a track-relative sector index.
    pub fn current_sector(&self) -> u32 {
        self.total_sectors - self.remaining_sectors
    }

    /// Seek to a track-relative sector position (valid range `0..=total_sectors()`).
    pub fn seek_to_sector(&mut self, sector: u32) -> Result<(), CdReaderError> {
        if sector > self.total_sectors {
            return Err(CdReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek sector is out of track bounds",
            )));
        }

        self.next_lba = self.start_lba + sector;
        self.remaining_sectors = self.total_sectors - sector;
        Ok(())
    }

    /// Current position in seconds (75 sectors = 1 second).
    pub fn current_seconds(&self) -> f32 {
        self.current_sector() as f32 / Self::SECTORS_PER_SECOND
    }

    /// Total track duration in seconds (75 sectors = 1 second).
    pub fn total_seconds(&self) -> f32 {
        self.total_sectors as f32 / Self::SECTORS_PER_SECOND
    }

    /// Seek to a track-relative time in seconds, clamped to the track length.
    pub fn seek_to_seconds(&mut self, seconds: f32) -> Result<(), CdReaderError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(CdReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek seconds must be a finite non-negative number",
            )));
        }

        let target_sector = (seconds * Self::SECTORS_PER_SECOND).round() as u32;
        self.seek_to_sector(target_sector.min(self.total_sectors))
    }
}

/// Open a streaming reader for a track using physical-disc geometry
/// ([`TrackBounds::PhysicalDisc`]). See [`AudioTrackStream`].
pub fn open_track_stream<'a, R: AudioSectorReader>(
    src: &'a R,
    toc: &Toc,
    track_no: u8,
) -> Result<AudioTrackStream<'a, R>, CdReaderError> {
    open_track_stream_with_bounds(src, toc, track_no, TrackBounds::PhysicalDisc)
}

/// Open a streaming reader for a track with an explicit [`TrackBounds`] geometry.
/// Use [`TrackBounds::GaplessImage`] for extracted CHD/BIN images.
pub fn open_track_stream_with_bounds<'a, R: AudioSectorReader>(
    src: &'a R,
    toc: &Toc,
    track_no: u8,
    bounds: TrackBounds,
) -> Result<AudioTrackStream<'a, R>, CdReaderError> {
    let (start_lba, sectors) = bounds.resolve(toc, track_no).map_err(CdReaderError::Io)?;
    Ok(AudioTrackStream::new(src, start_lba, sectors))
}

/// Open a streaming reader over an explicit absolute sector range
/// (`start_lba .. start_lba + sectors`), bypassing TOC bounds resolution.
///
/// For backings that compute their own track layout — e.g. a gapless CHD/BIN
/// image reading `[start_lba(n) .. start_lba(n + 1))` — this is the zero-policy
/// primitive: no TOC lookup, no CD-Extra rule, and no failure mode.
pub fn open_track_stream_at<R: AudioSectorReader>(
    src: &R,
    start_lba: u32,
    sectors: u32,
) -> AudioTrackStream<'_, R> {
    AudioTrackStream::new(src, start_lba, sectors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Track, create_wav, lba_to_msf};

    /// Minimal in-memory backing: whole-disc PCM sliced by sector.
    struct MemDisc {
        pcm: Vec<u8>,
    }

    impl AudioSectorReader for MemDisc {
        type Error = std::io::Error;

        fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
            let start = start_lba as usize * 2352;
            let end = start + count as usize * 2352;
            self.pcm.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "read past end of disc")
            })
        }
    }

    fn toc_two_tracks(t1_sectors: u32, t2_sectors: u32) -> Toc {
        Toc {
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
                    start_lba: t1_sectors,
                    start_msf: lba_to_msf(t1_sectors),
                    is_audio: true,
                },
            ],
            leadout_lba: t1_sectors + t2_sectors,
        }
    }

    #[test]
    fn reads_track_bytes_for_the_right_range() {
        let (t1, t2) = (100u32, 50u32);
        let disc = MemDisc {
            pcm: vec![0u8; (t1 + t2) as usize * 2352],
        };
        let toc = toc_two_tracks(t1, t2);

        let track1 = read_track(&disc, &toc, 1).unwrap();
        let track2 = read_track(&disc, &toc, 2).unwrap();

        assert_eq!(track1.len(), t1 as usize * 2352);
        assert_eq!(track2.len(), t2 as usize * 2352);
    }

    #[test]
    fn create_wav_wraps_backend_pcm() {
        let disc = MemDisc {
            pcm: vec![0u8; 10 * 2352],
        };
        // Single-track disc: no next track, so the leadout bounds the read.
        let toc = Toc {
            first_track: 1,
            last_track: 1,
            tracks: vec![Track {
                number: 1,
                start_lba: 0,
                start_msf: lba_to_msf(0),
                is_audio: true,
            }],
            leadout_lba: 10,
        };

        let pcm = read_track(&disc, &toc, 1).unwrap();
        let wav = create_wav(pcm);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 10 * 2352);
    }

    #[test]
    fn missing_track_is_an_io_error() {
        let disc = MemDisc {
            pcm: vec![0u8; 2352],
        };
        let toc = toc_two_tracks(1, 0);
        match read_track(&disc, &toc, 99) {
            Err(CdReaderError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            other => panic!("expected Io(NotFound), got {other:?}"),
        }
    }

    #[test]
    fn backend_failure_is_a_backend_error() {
        // Disc holds one sector but the TOC claims track 1 is five, so the read
        // runs past the end — a backing failure, not a TOC error.
        let disc = MemDisc {
            pcm: vec![0u8; 2352],
        };
        let toc = toc_two_tracks(5, 10);
        match read_track(&disc, &toc, 1) {
            Err(CdReaderError::Backend(e)) => {
                let io = e
                    .downcast_ref::<std::io::Error>()
                    .expect("backend error preserves the io::Error");
                assert_eq!(io.kind(), std::io::ErrorKind::UnexpectedEof);
            }
            other => panic!("expected Backend error, got {other:?}"),
        }
    }

    #[test]
    fn stream_pulls_sector_aligned_chunks() {
        let sectors = 100u32;
        let disc = MemDisc {
            pcm: vec![0u8; sectors as usize * 2352],
        };

        let mut stream = open_track_stream_at(&disc, 0, sectors).with_sectors_per_chunk(27);
        assert_eq!(stream.total_sectors(), sectors);

        let mut total = 0usize;
        let mut chunks = 0usize;
        while let Some(chunk) = stream.next_chunk().unwrap() {
            assert_eq!(chunk.len() % 2352, 0);
            total += chunk.len();
            chunks += 1;
        }

        assert_eq!(total, sectors as usize * 2352);
        assert_eq!(chunks, 4); // 27 + 27 + 27 + 19
        assert!(stream.next_chunk().unwrap().is_none());
    }

    #[test]
    fn stream_seek_repositions() {
        // Stream starts at absolute LBA 10 for 300 sectors, so the backing must
        // cover absolute sectors 10..310.
        let disc = MemDisc {
            pcm: vec![0u8; 310 * 2352],
        };
        let mut stream = open_track_stream_at(&disc, 10, 300).with_sectors_per_chunk(1000);

        stream.seek_to_sector(250).unwrap();
        assert_eq!(stream.current_sector(), 250);
        assert!((stream.current_seconds() - 250.0 / 75.0).abs() < f32::EPSILON);

        let chunk = stream.next_chunk().unwrap().unwrap();
        assert_eq!(chunk.len(), 50 * 2352); // 300 - 250 sectors, one big chunk
        assert!(stream.next_chunk().unwrap().is_none());

        assert!(stream.seek_to_sector(301).is_err());
    }

    #[test]
    fn open_track_stream_resolves_toc_bounds() {
        let (t1, t2) = (40u32, 60u32);
        let disc = MemDisc {
            pcm: vec![0u8; (t1 + t2) as usize * 2352],
        };
        let toc = toc_two_tracks(t1, t2);

        let stream = open_track_stream(&disc, &toc, 2).unwrap();
        assert_eq!(stream.total_sectors(), t2);
    }
}
