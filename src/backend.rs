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
//! your backing, build a [`Toc`](crate::Toc) from the image's own track
//! metadata (see [`lba_to_msf`](crate::lba_to_msf)), then call [`read_track`] to
//! get PCM in the exact same little-endian, 2352-byte/sector format the physical
//! reader produces — ready for [`create_wav`](crate::create_wav).
//!
//! See `examples/file_backend.rs` for a complete, dependency-free example.

use std::fmt;

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
pub trait AudioSectorReader {
    /// Error type produced by this backing.
    type Error;

    /// Read `count` sectors starting at absolute `start_lba`, returning exactly
    /// `count * 2352` bytes of little-endian PCM.
    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error>;
}

/// Error returned by [`read_track`].
///
/// Separates a bad track request (not in the TOC, or invalid TOC bounds) from a
/// failure inside the backing [`AudioSectorReader`], whose error type is
/// preserved as `E` rather than flattened into this crate's SCSI-oriented
/// [`CdReaderError`](crate::CdReaderError).
#[derive(Debug)]
pub enum TrackReadError<E> {
    /// The requested track number was not found in the TOC, or the resolved
    /// sector bounds were invalid.
    Toc(std::io::Error),
    /// The backing [`AudioSectorReader`] failed while reading sectors.
    Backend(E),
}

impl<E: fmt::Display> fmt::Display for TrackReadError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toc(err) => write!(f, "TOC error: {err}"),
            Self::Backend(err) => write!(f, "backend error: {err}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for TrackReadError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toc(err) => Some(err),
            Self::Backend(err) => Some(err),
        }
    }
}

/// Read raw PCM for one track from any [`AudioSectorReader`] backing.
///
/// This is the file/image counterpart to
/// [`CdReader::read_track`](crate::CdReader::read_track): it resolves the track's
/// sector range from `toc` (honouring the CD-Extra trailing-gap rule) and pulls
/// those sectors from `src`. The returned bytes are the same little-endian,
/// 2352-B/sector PCM, so [`create_wav`](crate::create_wav) wraps them into a
/// playable file unchanged.
pub fn read_track<R: AudioSectorReader>(
    src: &R,
    toc: &Toc,
    track_no: u8,
) -> Result<Vec<u8>, TrackReadError<R::Error>> {
    let (start_lba, sectors) =
        utils::get_track_bounds(toc, track_no).map_err(TrackReadError::Toc)?;
    src.read_audio_sectors(start_lba, sectors)
        .map_err(TrackReadError::Backend)
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
    fn missing_track_is_a_toc_error() {
        let disc = MemDisc {
            pcm: vec![0u8; 2352],
        };
        let toc = toc_two_tracks(1, 0);
        match read_track(&disc, &toc, 99) {
            Err(TrackReadError::Toc(_)) => {}
            other => panic!("expected TOC error, got {other:?}"),
        }
    }
}
