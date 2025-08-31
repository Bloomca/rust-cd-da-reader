use crate::Toc;

pub fn get_track_bounds(toc: &Toc, track_no: u8) -> std::io::Result<(u32, u32)> {
    let idx = toc
        .tracks
        .iter()
        .position(|t| t.number == track_no)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "track not in TOC"))?;

    let start_lba = toc.tracks[idx].start_lba;

    // Determine end LBA (next track start, or lead-out for the last track)
    let end_lba: u32 = if (idx + 1) < toc.tracks.len() {
        toc.tracks[idx + 1].start_lba
    } else {
        toc.leadout_lba
    };

    if end_lba <= start_lba {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad TOC bounds",
        ));
    }

    let sectors = end_lba - start_lba;

    Ok((start_lba, sectors))
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
    fn returns_error_for_invalid_track() {
        let toc = get_toc();

        let result = get_track_bounds(&toc, 100);
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
