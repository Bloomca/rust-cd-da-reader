mod detect;
mod raw_sector;
mod sector_read_format;
pub(crate) mod track_information;

pub use sector_read_format::SectorReadFormat;

use crate::retry::RetryConfig;
use crate::{CdReaderError, Track};

/// Sector format and retry options for track and sector-range reads.
///
/// The defaults read audio sectors using the default retry policy. Use the
/// builder methods to override only the options you need.
#[derive(Debug, Clone)]
pub struct ReadOptions {
    format: SectorReadFormat,
    retry: RetryConfig,
}

impl ReadOptions {
    /// Select the sector format requested from the drive.
    pub fn with_format(mut self, format: SectorReadFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the retry policy applied to each read command.
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    pub(crate) fn format(&self) -> SectorReadFormat {
        self.format
    }

    pub(crate) fn retry(&self) -> &RetryConfig {
        &self.retry
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            format: SectorReadFormat::Audio,
            retry: RetryConfig::default(),
        }
    }
}

pub(crate) fn validate_track_format(
    track: &Track,
    format: SectorReadFormat,
) -> Result<(), CdReaderError> {
    // this checks both whether both are audio, or both are not audio
    if track.is_audio == format.is_audio() {
        return Ok(());
    }

    Err(CdReaderError::TrackFormatMismatch {
        track_number: track.number,
        track_is_audio: track.is_audio,
        requested_format: format,
    })
}

/// Build a READ CD (0xBE) command descriptor block for Linux and Windows.
#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub(crate) fn build_read_cd_cdb(lba: u32, sectors: u32, format: SectorReadFormat) -> [u8; 12] {
    let mut cdb = [0u8; 12];
    cdb[0] = 0xBE;
    cdb[1] = format.cdb_byte1();
    cdb[2..6].copy_from_slice(&lba.to_be_bytes());
    cdb[6] = ((sectors >> 16) & 0xFF) as u8;
    cdb[7] = ((sectors >> 8) & 0xFF) as u8;
    cdb[8] = (sectors & 0xFF) as u8;
    cdb[9] = format.cdb_byte9();
    cdb
}

#[cfg(test)]
mod tests {
    use super::{ReadOptions, SectorReadFormat, build_read_cd_cdb, validate_track_format};
    use crate::{CdReaderError, Track};

    #[test]
    fn read_options_builders_override_individual_defaults() {
        assert_eq!(ReadOptions::default().format(), SectorReadFormat::Audio);

        let retry = crate::RetryConfig::default().with_max_attempts(9);
        let options = ReadOptions::default()
            .with_format(SectorReadFormat::Mode1Raw)
            .with_retry(retry);

        assert_eq!(options.format(), SectorReadFormat::Mode1Raw);
        assert_eq!(options.retry().max_attempts, 9);
    }

    #[test]
    fn validates_track_and_format_compatibility() {
        let audio = Track {
            number: 1,
            start_lba: 0,
            start_msf: (0, 2, 0),
            is_audio: true,
        };
        let data = Track {
            number: 2,
            start_lba: 10_000,
            start_msf: (2, 15, 25),
            is_audio: false,
        };

        assert!(validate_track_format(&audio, SectorReadFormat::Audio).is_ok());
        assert!(validate_track_format(&data, SectorReadFormat::Mode1Cooked).is_ok());
        assert!(validate_track_format(&data, SectorReadFormat::Mode1Raw).is_ok());
        assert!(validate_track_format(&data, SectorReadFormat::Mode2Raw).is_ok());

        assert!(matches!(
            validate_track_format(&audio, SectorReadFormat::Mode1Cooked),
            Err(CdReaderError::TrackFormatMismatch {
                track_number: 1,
                track_is_audio: true,
                requested_format: SectorReadFormat::Mode1Cooked,
            })
        ));
        assert!(matches!(
            validate_track_format(&data, SectorReadFormat::Audio),
            Err(CdReaderError::TrackFormatMismatch {
                track_number: 2,
                track_is_audio: false,
                requested_format: SectorReadFormat::Audio,
            })
        ));
    }

    #[test]
    fn builds_read_cd_cdb() {
        assert_eq!(
            build_read_cd_cdb(0x1234_5678, 0x0000_ABCD, SectorReadFormat::Mode1Raw),
            [
                0xBE, 0x08, 0x12, 0x34, 0x56, 0x78, 0x00, 0xAB, 0xCD, 0xF8, 0x00, 0x00,
            ]
        );
    }
}
