use crate::Toc;

const CD_EXTRA_TRAILING_DATA_GAP_SECTORS: u32 = 11_400;

pub fn get_track_bounds(toc: &Toc, track_no: u8) -> std::io::Result<(u32, u32)> {
    let idx = toc
        .tracks
        .iter()
        .position(|t| t.number == track_no)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "track not in TOC"))?;

    let start_lba = toc.tracks[idx].start_lba;
    let end_lba = get_track_end_lba(toc, idx)?;

    if end_lba <= start_lba {
        return Err(bad_toc_bounds());
    }

    let sectors = end_lba - start_lba;

    Ok((start_lba, sectors))
}

fn get_track_end_lba(toc: &Toc, idx: usize) -> std::io::Result<u32> {
    if is_cd_extra_audio_session_boundary(toc, idx) {
        return toc.tracks[idx + 1]
            .start_lba
            .checked_sub(CD_EXTRA_TRAILING_DATA_GAP_SECTORS)
            .ok_or_else(bad_toc_bounds);
    }

    if (idx + 1) < toc.tracks.len() {
        Ok(toc.tracks[idx + 1].start_lba)
    } else {
        Ok(toc.leadout_lba)
    }
}

fn is_cd_extra_audio_session_boundary(toc: &Toc, idx: usize) -> bool {
    toc.tracks[idx].is_audio
        && (idx + 1) < toc.tracks.len()
        && toc.tracks[idx + 1..].iter().all(|track| !track.is_audio)
}

fn bad_toc_bounds() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, "bad TOC bounds")
}

pub fn create_wav_header(pcm_data_size: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(44);

    // RIFF header
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(pcm_data_size + 36).to_le_bytes()); // file size - 8
    header.extend_from_slice(b"WAVE");

    // fmt chunk
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    header.extend_from_slice(&2u16.to_le_bytes()); // channels
    header.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
    header.extend_from_slice(&176400u32.to_le_bytes()); // byte rate
    header.extend_from_slice(&4u16.to_le_bytes()); // block align
    header.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk header
    header.extend_from_slice(b"data");
    header.extend_from_slice(&pcm_data_size.to_le_bytes());

    header
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Track;

    fn get_toc() -> Toc {
        Toc {
            first_track: 1,
            last_track: 11,
            tracks: vec![
                Track {
                    number: 1,
                    start_lba: 0,
                    start_msf: (0, 2, 0),
                    is_audio: true,
                },
                Track {
                    number: 2,
                    start_lba: 13132,
                    start_msf: (2, 57, 7),
                    is_audio: true,
                },
                Track {
                    number: 3,
                    start_lba: 27967,
                    start_msf: (6, 14, 67),
                    is_audio: true,
                },
                Track {
                    number: 4,
                    start_lba: 47464,
                    start_msf: (10, 34, 64),
                    is_audio: true,
                },
                Track {
                    number: 5,
                    start_lba: 63025,
                    start_msf: (14, 2, 25),
                    is_audio: true,
                },
                Track {
                    number: 6,
                    start_lba: 90420,
                    start_msf: (20, 7, 45),
                    is_audio: true,
                },
                Track {
                    number: 7,
                    start_lba: 104142,
                    start_msf: (23, 10, 42),
                    is_audio: true,
                },
                Track {
                    number: 8,
                    start_lba: 126725,
                    start_msf: (28, 11, 50),
                    is_audio: true,
                },
                Track {
                    number: 9,
                    start_lba: 139887,
                    start_msf: (31, 7, 12),
                    is_audio: true,
                },
                Track {
                    number: 10,
                    start_lba: 164252,
                    start_msf: (36, 32, 2),
                    is_audio: true,
                },
                Track {
                    number: 11,
                    start_lba: 179485,
                    start_msf: (39, 55, 10),
                    is_audio: true,
                },
            ],
            leadout_lba: 204855,
        }
    }

    fn track(number: u8, start_lba: u32, is_audio: bool) -> Track {
        Track {
            number,
            start_lba,
            start_msf: (0, 0, 0),
            is_audio,
        }
    }

    #[test]
    fn finds_non_last_track_bounds_correctly() {
        let toc = get_toc();

        let result = get_track_bounds(&toc, 5);
        assert!(result.is_ok());
        let (start_lba, sectors) = result.unwrap();

        assert_eq!(start_lba, 63025);
        assert_eq!(sectors, 90420 - 63025);
    }

    #[test]
    fn finds_last_track_bounds_correctly() {
        let toc = get_toc();

        let result = get_track_bounds(&toc, 11);
        assert!(result.is_ok());
        let (start_lba, sectors) = result.unwrap();
        assert_eq!(start_lba, 179485);
        assert_eq!(sectors, 204855 - 179485);
    }

    #[test]
    fn subtracts_cd_extra_gap_for_last_audio_track_before_trailing_data_tracks() {
        let toc = Toc {
            first_track: 1,
            last_track: 4,
            tracks: vec![
                track(1, 0, true),
                track(2, 10_000, true),
                track(3, 40_000, false),
                track(4, 80_000, false),
            ],
            leadout_lba: 120_000,
        };

        let result = get_track_bounds(&toc, 2);
        assert!(result.is_ok());
        let (start_lba, sectors) = result.unwrap();

        assert_eq!(start_lba, 10_000);
        assert_eq!(
            sectors,
            (40_000 - CD_EXTRA_TRAILING_DATA_GAP_SECTORS) - 10_000
        );
    }

    #[test]
    fn does_not_subtract_cd_extra_gap_when_audio_track_follows_later() {
        let toc = Toc {
            first_track: 1,
            last_track: 4,
            tracks: vec![
                track(1, 0, true),
                track(2, 10_000, true),
                track(3, 40_000, false),
                track(4, 80_000, true),
            ],
            leadout_lba: 120_000,
        };

        let result = get_track_bounds(&toc, 2);
        assert!(result.is_ok());
        let (start_lba, sectors) = result.unwrap();

        assert_eq!(start_lba, 10_000);
        assert_eq!(sectors, 40_000 - 10_000);
    }

    #[test]
    fn returns_error_for_invalid_track() {
        let toc = get_toc();

        let result = get_track_bounds(&toc, 100);
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
