use crate::retry::RetryConfig;

/// Sector format and retry options for track and sector-range reads.
///
/// The defaults read audio sectors using the default retry policy. Use the
/// builder methods to override only the options you need.
#[derive(Debug, Clone)]
pub struct ReadOptions {
    mode: SectorReadMode,
    retry: RetryConfig,
}

impl ReadOptions {
    /// Select the sector format requested from the drive.
    pub fn with_mode(mut self, mode: SectorReadMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the retry policy applied to each read command.
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    pub(crate) fn mode(&self) -> SectorReadMode {
        self.mode
    }

    pub(crate) fn retry(&self) -> &RetryConfig {
        &self.retry
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            mode: SectorReadMode::Audio,
            retry: RetryConfig::default(),
        }
    }
}

/// Sector read mode for the READ CD (0xBE) command.
///
/// Controls CDB byte 1 (Expected Sector Type) and byte 9 (Main Channel Selection)
/// to read different sector formats from the same READ CD command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorReadMode {
    /// Audio: 2352 bytes/sector, raw PCM.
    /// CDB byte 1 = 0x00 (any type), byte 9 = 0x10 (user data).
    Audio,
    /// Data cooked: 2048 bytes/sector, user data only (no sync/header/EDC/ECC).
    /// CDB byte 1 = 0x08 (Mode 1), byte 9 = 0x10 (user data).
    DataCooked,
    /// Data raw: 2352 bytes/sector with sync + header + user data + EDC/ECC.
    /// CDB byte 1 = 0x08 (Mode 1), byte 9 = 0xF8 (sync + header + user data + EDC/ECC).
    DataRaw,
}

impl SectorReadMode {
    /// Bytes per sector for this read mode.
    pub fn sector_size(&self) -> usize {
        match self {
            SectorReadMode::Audio => 2352,
            SectorReadMode::DataCooked => 2048,
            SectorReadMode::DataRaw => 2352,
        }
    }

    /// CDB byte 1: Expected Sector Type field (bits 4-2).
    ///
    /// Per SCSI MMC, the type value is shifted left by 2: Mode 1 is
    /// `010b << 2 = 0x08`. (`0x04` would be `001b << 2`, i.e. CD-DA.)
    pub fn cdb_byte1(&self) -> u8 {
        match self {
            SectorReadMode::Audio => 0x00,
            SectorReadMode::DataCooked => 0x08,
            SectorReadMode::DataRaw => 0x08,
        }
    }

    /// CDB byte 9: Main Channel Selection bits.
    pub fn cdb_byte9(&self) -> u8 {
        match self {
            SectorReadMode::Audio => 0x10,
            SectorReadMode::DataCooked => 0x10,
            SectorReadMode::DataRaw => 0xF8,
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
pub(crate) fn build_read_cd_cdb(lba: u32, sectors: u32, mode: SectorReadMode) -> [u8; 12] {
    let mut cdb = [0u8; 12];
    cdb[0] = 0xBE;
    cdb[1] = mode.cdb_byte1();
    cdb[2..6].copy_from_slice(&lba.to_be_bytes());
    cdb[6] = ((sectors >> 16) & 0xFF) as u8;
    cdb[7] = ((sectors >> 8) & 0xFF) as u8;
    cdb[8] = (sectors & 0xFF) as u8;
    cdb[9] = mode.cdb_byte9();
    cdb
}

#[cfg(test)]
mod tests {
    use super::{ReadOptions, SectorReadMode, build_read_cd_cdb};

    #[test]
    fn read_options_builders_override_individual_defaults() {
        assert_eq!(ReadOptions::default().mode(), SectorReadMode::Audio);

        let retry = crate::RetryConfig {
            max_attempts: 9,
            ..crate::RetryConfig::default()
        };
        let options = ReadOptions::default()
            .with_mode(SectorReadMode::DataRaw)
            .with_retry(retry);

        assert_eq!(options.mode(), SectorReadMode::DataRaw);
        assert_eq!(options.retry().max_attempts, 9);
    }

    #[test]
    fn cdb_byte1_encodes_expected_sector_type() {
        // Expected Sector Type lives in CDB byte 1, bits 4-2 (value << 2).
        // Audio leaves it 0 (any type) and relies on byte 9; data reads must
        // select Mode 1 = 010b << 2 = 0x08 (0x04 would wrongly mean CD-DA).
        assert_eq!(SectorReadMode::Audio.cdb_byte1(), 0x00);
        assert_eq!(SectorReadMode::DataCooked.cdb_byte1(), 0x08);
        assert_eq!(SectorReadMode::DataRaw.cdb_byte1(), 0x08);
    }

    #[test]
    fn cdb_byte9_selects_main_channel() {
        assert_eq!(SectorReadMode::Audio.cdb_byte9(), 0x10);
        assert_eq!(SectorReadMode::DataCooked.cdb_byte9(), 0x10);
        assert_eq!(SectorReadMode::DataRaw.cdb_byte9(), 0xF8);
    }

    #[test]
    fn sector_size_matches_mode() {
        assert_eq!(SectorReadMode::Audio.sector_size(), 2352);
        assert_eq!(SectorReadMode::DataCooked.sector_size(), 2048);
        assert_eq!(SectorReadMode::DataRaw.sector_size(), 2352);
    }

    #[test]
    fn builds_read_cd_cdb() {
        assert_eq!(
            build_read_cd_cdb(0x1234_5678, 0x0000_ABCD, SectorReadMode::DataRaw),
            [
                0xBE, 0x08, 0x12, 0x34, 0x56, 0x78, 0x00, 0xAB, 0xCD, 0xF8, 0x00, 0x00,
            ]
        );
    }
}
