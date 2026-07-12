use crate::retry::RetryConfig;

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

/// Sector format requested through the READ CD (0xBE) command.
///
/// Controls CDB byte 1 (Expected Sector Type) and byte 9 (Main Channel Selection)
/// to return audio, cooked Mode 1 data, or complete Mode 1 sectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorReadFormat {
    /// Audio: 2352 bytes/sector, raw PCM.
    /// CDB byte 1 = 0x00 (any type), byte 9 = 0x10 (user data).
    Audio,
    /// Mode 1 cooked data: 2048 bytes/sector, containing user data only.
    /// CDB byte 1 = 0x08 (Mode 1), byte 9 = 0x10 (user data).
    Mode1Cooked,
    /// Complete Mode 1 sector: 2352 bytes with sync, header, user data, EDC, and ECC.
    /// CDB byte 1 = 0x08 (Mode 1), byte 9 = 0xF8 (all main-channel fields).
    Mode1Raw,
}

impl SectorReadFormat {
    /// Bytes per sector for this read format.
    pub fn sector_size(&self) -> usize {
        match self {
            SectorReadFormat::Audio => 2352,
            SectorReadFormat::Mode1Cooked => 2048,
            SectorReadFormat::Mode1Raw => 2352,
        }
    }

    /// CDB byte 1: Expected Sector Type field (bits 4-2).
    ///
    /// Per SCSI MMC, the type value is shifted left by 2: Mode 1 is
    /// `010b << 2 = 0x08`. (`0x04` would be `001b << 2`, i.e. CD-DA.)
    pub fn cdb_byte1(&self) -> u8 {
        match self {
            SectorReadFormat::Audio => 0x00,
            SectorReadFormat::Mode1Cooked => 0x08,
            SectorReadFormat::Mode1Raw => 0x08,
        }
    }

    /// CDB byte 9: Main Channel Selection bits.
    pub fn cdb_byte9(&self) -> u8 {
        match self {
            SectorReadFormat::Audio => 0x10,
            SectorReadFormat::Mode1Cooked => 0x10,
            SectorReadFormat::Mode1Raw => 0xF8,
        }
    }

    /// Maximum sectors per single `READ CD` command.
    ///
    /// This is the sole chunker for the blocking `read_track` /
    /// `read_sector_range` paths, which can hand a whole track (tens of thousands
    /// of sectors) straight to the read loop; the streaming API already limits
    /// itself via its `TrackStreamOptions` chunk size. The cap is not about
    /// OS pass-through limits (modern SG_IO/SPTI handle far larger transfers)
    /// but about optical-drive firmware and USB-bridge reliability: large
    /// multi-sector `READ CD` requests are flaky across the zoo of drives. The
    /// values keep each transfer around 64 KiB, matching the conventional
    /// ~27-sector chunk used by cdparanoia/libcdio.
    pub(crate) fn max_sectors_per_xfer(&self) -> u32 {
        match self.sector_size() {
            2048 => 32, // 32 * 2048 = 64 KiB
            _ => 27,    // 27 * 2352 ≈ 62 KiB
        }
    }
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
    use super::{ReadOptions, SectorReadFormat, build_read_cd_cdb};

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
    fn cdb_byte1_encodes_expected_sector_type() {
        // Expected Sector Type lives in CDB byte 1, bits 4-2 (value << 2).
        // Audio leaves it 0 (any type) and relies on byte 9; Mode 1 reads use
        // 010b << 2 = 0x08 (0x04 would mean CD-DA).
        assert_eq!(SectorReadFormat::Audio.cdb_byte1(), 0x00);
        assert_eq!(SectorReadFormat::Mode1Cooked.cdb_byte1(), 0x08);
        assert_eq!(SectorReadFormat::Mode1Raw.cdb_byte1(), 0x08);
    }

    #[test]
    fn cdb_byte9_selects_main_channel() {
        assert_eq!(SectorReadFormat::Audio.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode1Cooked.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode1Raw.cdb_byte9(), 0xF8);
    }

    #[test]
    fn sector_size_matches_format() {
        assert_eq!(SectorReadFormat::Audio.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Mode1Cooked.sector_size(), 2048);
        assert_eq!(SectorReadFormat::Mode1Raw.sector_size(), 2352);
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
