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
//! ## Track bounds and the CD-Extra gap
//!
//! Resolving a track's sector range from a [`Toc`] differs on exactly one track:
//! the last audio track before a trailing data session on a CD-Extra disc. There
//! the crate subtracts the inter-session gap — correct whenever that gap is part
//! of the addressing (a physical disc, or an image whose TOC preserves the real
//! LBAs), but wrong when the tracks are addressed back-to-back with the gap
//! stripped out (a `chdman extractcd`-style contiguous extract), where it would
//! drop ~2.5 min of real audio. Only the backing knows its own layout, so
//! [`read_track`] / [`open_track_stream`] default to [`TrackBounds::SessionGap`];
//! a contiguous backing must pass [`TrackBounds::Gapless`] (or supply explicit
//! bounds via [`open_track_stream_at`]).
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
/// This trait separates source-specific I/O from the crate's track-level logic.
/// Meaning that you can provide your implementation for any source which can provide
/// audio CD sectors, like a disc image, decoded container, in-memory disc, or a remote
/// source. [`read_track`] and [`open_track_stream`] use a caller-provided [`Toc`] to calculate
/// sector ranges, then retrieve those sectors through [`read_audio_sectors`](Self::read_audio_sectors).
/// 
/// Implementations are responsible only for reading sectors. They do not build
/// the [`Toc`], select tracks, calculate track boundaries, or account for
/// CD-Extra session gaps. The backing's sector address space must agree with the
/// `start_lba` and `leadout_lba` values in the supplied `Toc`; layout differences
/// are expressed separately through [`TrackBounds`].
///
/// # Audio format
///
/// Each sector must contain exactly 2,352 bytes of headerless PCM audio:
///
/// - 44,100 sample frames per second
/// - signed 16-bit little-endian samples
/// - two interleaved channels, left followed by right
/// - 588 stereo sample frames per sector
///
/// One sector therefore represents 1/75 second of audio. Returned data must not
/// include a WAV header, CD sector headers, subchannel data, or padding. It is
/// byte-for-byte compatible with [`CdReader::read_track`] and can be passed
/// directly to [`create_wav`].
///
/// # Addressing and read semantics
///
/// `start_lba` is an absolute sector index within the backing, not an offset
/// relative to a track. A request covers the half-open range
/// `start_lba..start_lba + count`.
///
/// Calls are independent and may be repeated or issued out of order, such as
/// after seeking a stream. On success, the returned vector must contain exactly
/// `count * 2352` bytes. A zero-sector request should return an empty vector.
/// Invalid ranges, short reads, and decoding or I/O failures must return an
/// error rather than partial data.
///
/// The method takes `&self` so callers can retain a shared reference to the
/// source. Implementations backed by a mutable file cursor or decoder should
/// use positioned reads or interior mutability.
pub trait AudioSectorReader {
    /// Error produced when this backing cannot satisfy a sector read.
    ///
    /// Helper APIs preserve this error as the source of [`CdReaderError::Backend`].
    type Error: std::error::Error + Send + Sync + 'static;

    /// Read the sector range `start_lba..start_lba + count`.
    ///
    /// A successful call returns exactly `count * 2352` bytes in the format
    /// described by [`AudioSectorReader`].
    ///
    /// # Errors
    ///
    /// Returns an error if the complete requested range cannot be returned.
    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error>;
}

/// Policy for deriving a track's half-open sector range from a [`Toc`].
///
/// A track begins at its own [`Track::start_lba`](crate::Track::start_lba).
/// Normally it ends at the next track's `start_lba`, or at
/// [`Toc::leadout_lba`] when it is the final track.
///
/// # CD-Extra session gaps
///
/// A CD-Extra disc places a standard 11,400-sector inter-session gap between
/// its final audio track and the following data session. This is 152 seconds,
/// or 2 minutes 32 seconds. In a geometry-preserving address space, the first
/// data track's `start_lba` lies after that gap, so treating it as the audio
/// track's end would incorrectly include the gap in the audio range.
///
/// Some image formats and extracts remove the inter-session gap and store the
/// tracks back-to-back. In that layout, the next track's `start_lba` is already
/// the correct end of the audio track; subtracting 11,400 sectors would instead
/// truncate 152 seconds of audio.
///
/// This policy affects only the last audio track followed exclusively by data
/// tracks. All other tracks have identical bounds under both variants.
///
/// Choose the variant according to the address space represented jointly by the
/// backing and its `Toc`:
///
/// - [`SessionGap`](Self::SessionGap) for a physical disc or an image that
///   preserves the disc's original sector geometry.
/// - [`Gapless`](Self::Gapless) for a contiguous, gap-stripped image or extract.
///
/// `Gapless` refers only to the CD-Extra inter-session gap. It does not remove
/// ordinary track pregaps or provide gapless playback.
///
/// [`read_track`] and [`open_track_stream`] use [`SessionGap`](Self::SessionGap)
/// by default. Use [`read_track_with_bounds`] or
/// [`open_track_stream_with_bounds`] when the source requires an explicit
/// policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackBounds {
    /// The address space includes the CD-Extra inter-session gap.
    ///
    /// When applicable, the final audio track ends 11,400 sectors before the
    /// following data track.
    SessionGap,
    /// The address space stores tracks contiguously without the CD-Extra
    /// inter-session gap.
    ///
    /// Every track ends at the next track's start, or at the lead-out.
    Gapless,
}

impl TrackBounds {
    fn resolve(self, toc: &Toc, track_no: u8) -> std::io::Result<(u32, u32)> {
        match self {
            TrackBounds::SessionGap => utils::get_track_bounds(toc, track_no),
            TrackBounds::Gapless => utils::get_gapless_track_bounds(toc, track_no),
        }
    }
}

/// Reads one complete audio track from an [`AudioSectorReader`] into memory.
///
/// `track_no` is the disc track number stored in [`Track::number`](crate::Track::number),
/// not an index into [`Toc::tracks`](crate::Toc::tracks). The source and the
/// `Toc` must use the same LBA address space.
///
/// This convenience function resolves the track's sector range using
/// [`TrackBounds::SessionGap`]. That policy is appropriate for physical discs
/// and images that preserve the original CD geometry, including the CD-Extra
/// inter-session gap. For a contiguous, gap-stripped source, use
/// [`read_track_with_bounds`] with [`TrackBounds::Gapless`].
///
/// The returned vector contains headerless CD-DA PCM in the format required by
/// [`AudioSectorReader`]: signed 16-bit little-endian stereo at 44.1 kHz, with
/// 2,352 bytes per sector. It can be passed directly to
/// [`create_wav`](crate::create_wav).
///
/// This is a blocking operation that buffers the entire track, which may require
/// hundreds of megabytes. Use [`open_track_stream`] or
/// [`open_track_stream_with_bounds`] to process the track incrementally.
///
/// Only audio tracks are meaningful for this API. Callers should select a track
/// whose [`Track::is_audio`](crate::Track::is_audio) field is `true`.
///
/// # Errors
///
/// Returns [`CdReaderError::Io`] if `track_no` is absent from the `Toc` or its
/// calculated sector bounds are invalid.
///
/// Returns [`CdReaderError::Backend`] if the source cannot read the requested
/// sectors. The source's original error is preserved as the boxed
/// [`source`](std::error::Error::source).
pub fn read_track<R: AudioSectorReader>(
    src: &R,
    toc: &Toc,
    track_no: u8,
) -> Result<Vec<u8>, CdReaderError> {
    read_track_with_bounds(src, toc, track_no, TrackBounds::SessionGap)
}

/// Reads one complete audio track into memory using an explicit [`TrackBounds`] policy.
///
/// This is the configurable form of [`read_track`], which always uses
/// [`TrackBounds::SessionGap`]. The `bounds` argument controls how the track's
/// end LBA is calculated from the `Toc`, specifically whether the CD-Extra
/// inter-session gap is present in the source's address space.
///
/// Use [`TrackBounds::SessionGap`] for a physical disc or geometry-preserving
/// image. Use [`TrackBounds::Gapless`] for a contiguous, gap-stripped source.
///
/// The source and `Toc` must use the same LBA address space. All other behavior,
/// including the returned PCM format and whole-track buffering, is identical to
/// [`read_track`]. Use [`open_track_stream_with_bounds`] to process the track
/// incrementally with an explicit bounds policy.
///
/// # Errors
///
/// Returns [`CdReaderError::Io`] if the track is absent from the `Toc` or its
/// calculated sector bounds are invalid.
///
/// Returns [`CdReaderError::Backend`] if the source cannot read the requested
/// sectors. The source's original error is preserved as the error's
/// [`source`](std::error::Error::source).
pub fn read_track_with_bounds<R: AudioSectorReader>(
    src: &R,
    toc: &Toc,
    track_no: u8,
    bounds: TrackBounds,
) -> Result<Vec<u8>, CdReaderError> {
    let (start_lba, sectors) = bounds.resolve(toc, track_no).map_err(CdReaderError::Io)?;
    src.read_audio_sectors(start_lba, sectors)
        .map_err(|e| CdReaderError::Backend(Box::new(e)))
}

/// A pull-based, sector-aligned stream of raw CD-DA PCM from an [`AudioSectorReader`].
///
/// An `AudioTrackStream` borrows its source and represents a fixed sector
/// range. Each call to [`next_chunk`](Self::next_chunk) synchronously reads and
/// returns the next portion of that range. Once all sectors have been consumed,
/// it returns `Ok(None)`.
///
/// Unlike [`read_track`], the stream does not allocate or retain the entire
/// track. Callers can process and discard each returned chunk before requesting
/// the next one. Chunks contain complete CD-DA sectors in the format specified
/// by [`AudioSectorReader`]; the final chunk may contain fewer sectors than the
/// configured chunk size.
///
/// The chunk size can be changed with [`with_sectors_per_chunk`](Self::with_sectors_per_chunk).
/// Stream position is relative to the beginning of its sector range and can be inspected
/// or changed with [`current_sector`](Self::current_sector), [`seek_to_sector`](Self::seek_to_sector),
/// and [`seek_to_seconds`](Self::seek_to_seconds).
///
/// Create a stream with:
///
/// - [`open_track_stream`] to resolve a track from a `Toc` using
///   [`TrackBounds::SessionGap`].
/// - [`open_track_stream_with_bounds`] to resolve a track using an explicit
///   [`TrackBounds`] policy.
/// - [`open_track_stream_at`] to stream an explicit absolute sector range
///   without consulting a `Toc`.
///
/// This is the source-independent audio counterpart to [`TrackStream`](crate::TrackStream),
/// which is tied to [`CdReader`] and supports drive-specific read options and data-sector formats.
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
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Backend`] if the backing read fails. The stream
    /// position does not advance on error, so a retry re-reads the same chunk.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CdReaderError> {
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
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] containing
    /// [`std::io::ErrorKind::InvalidInput`] if `sector` exceeds the track
    /// length.
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
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] containing
    /// [`std::io::ErrorKind::InvalidInput`] if `seconds` is negative or not
    /// finite.
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

/// Open a streaming reader for a track assuming the TOC includes the inter-session
/// gap ([`TrackBounds::SessionGap`]). See [`AudioTrackStream`].
///
/// # Errors
///
/// Returns [`CdReaderError::Io`] if the track is absent or its bounds are
/// invalid.
pub fn open_track_stream<'a, R: AudioSectorReader>(
    src: &'a R,
    toc: &Toc,
    track_no: u8,
) -> Result<AudioTrackStream<'a, R>, CdReaderError> {
    open_track_stream_with_bounds(src, toc, track_no, TrackBounds::SessionGap)
}

/// Open a streaming reader for a track with an explicit [`TrackBounds`] geometry.
/// Use [`TrackBounds::Gapless`] for a contiguous, gap-stripped layout.
///
/// # Errors
///
/// Returns [`CdReaderError::Io`] if the track is absent or its bounds are
/// invalid.
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
/// For backings that compute their own track layout — e.g. reading
/// `[start_lba(n) .. start_lba(n + 1))` from a contiguous extract — this is the
/// zero-policy primitive: no TOC lookup, no CD-Extra rule, and no failure mode.
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
