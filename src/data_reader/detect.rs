use crate::{CdReader, CdReaderError, SectorReadFormat, Track};

impl CdReader {
    /// Detect the default read format for a track.
    ///
    /// Audio tracks are identified directly from the TOC. Data tracks are
    /// queried with MMC READ TRACK INFORMATION and mapped to their cooked
    /// sector representation.
    pub fn detect_track_format(&self, track: &Track) -> Result<SectorReadFormat, CdReaderError> {
        if track.is_audio {
            return Ok(SectorReadFormat::Audio);
        }

        let information = self.drive.read_track_information(track.number)?;
        format_from_data_mode(information.data_mode).ok_or(CdReaderError::CannotDetectTrackFormat {
            track_number: track.number,
            data_mode: Some(information.data_mode),
        })
    }
}

/// Map the MMC READ TRACK INFORMATION Data Mode field to an unambiguous
/// default read format.
///
/// MMC reports `0x01` for Mode 1 and `0x02` for Mode 2. Track Information does
/// not identify the per-sector Mode 2 form, so Mode 2 is exposed as complete
/// raw sectors.
fn format_from_data_mode(data_mode: u8) -> Option<SectorReadFormat> {
    match data_mode {
        0x01 => Some(SectorReadFormat::Mode1Cooked),
        0x02 => Some(SectorReadFormat::Mode2Raw),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_tracks_do_not_require_drive_detection() {
        let reader = CdReader::test_reader();
        let track = Track {
            number: 1,
            start_lba: 0,
            start_msf: (0, 2, 0),
            is_audio: true,
        };

        assert_eq!(
            reader.detect_track_format(&track).unwrap(),
            SectorReadFormat::Audio
        );
    }

    #[test]
    fn maps_mode1_to_its_cooked_format() {
        assert_eq!(
            format_from_data_mode(0x01),
            Some(SectorReadFormat::Mode1Cooked)
        );
    }

    #[test]
    fn maps_mode2_to_raw_sectors() {
        assert_eq!(
            format_from_data_mode(0x02),
            Some(SectorReadFormat::Mode2Raw)
        );
    }

    #[test]
    fn rejects_unknown_or_reserved_mmc_data_modes() {
        assert_eq!(format_from_data_mode(0x00), None);
        for data_mode in 0x03..=0x0F {
            assert_eq!(format_from_data_mode(data_mode), None);
        }
    }
}
