/// Sector format requested through the READ CD (0xBE) command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorReadFormat {
    /// CD-DA audio: 2352 bytes of PCM per sector.
    Audio,
    /// Mode 1 user data only: 2048 bytes per sector.
    Mode1Cooked,
    /// Complete Mode 1 sector: 2352 bytes with sync, header, user data, EDC,
    /// and ECC.
    Mode1Raw,
    /// Complete Mode 2 sector: 2352 bytes.
    ///
    /// Mode 2 forms are a per-sector property. Consumers that need the
    /// application payload must inspect each sector's XA subheader.
    Mode2Raw,
}

impl SectorReadFormat {
    /// Bytes returned per sector for this format.
    pub fn sector_size(&self) -> usize {
        match self {
            Self::Audio | Self::Mode1Raw | Self::Mode2Raw => 2352,
            Self::Mode1Cooked => 2048,
        }
    }

    /// CDB byte 1: Expected Sector Type in bits 4–2.
    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub(crate) fn cdb_byte1(&self) -> u8 {
        match self {
            Self::Audio => 0x04,
            Self::Mode1Cooked | Self::Mode1Raw => 0x08,
            // Mode 2 forms can be interleaved, so let the drive determine the
            // actual sector type while returning the complete sector.
            Self::Mode2Raw => 0x00,
        }
    }

    /// CDB byte 9: Main Channel Selection.
    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub(crate) fn cdb_byte9(&self) -> u8 {
        match self {
            Self::Audio | Self::Mode1Cooked => 0x10,
            Self::Mode1Raw | Self::Mode2Raw => 0xF8,
        }
    }

    /// Maximum sectors per single `READ CD` command.
    ///
    /// Transfers are kept at approximately 64 KiB for compatibility with
    /// optical-drive firmware and USB bridges.
    pub(crate) fn max_sectors_per_xfer(&self) -> u32 {
        (64 * 1024 / self.sector_size() as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::SectorReadFormat;

    #[test]
    fn expected_sector_types_are_encoded_in_cdb_byte1() {
        assert_eq!(SectorReadFormat::Audio.cdb_byte1(), 0x04);
        assert_eq!(SectorReadFormat::Mode1Cooked.cdb_byte1(), 0x08);
        assert_eq!(SectorReadFormat::Mode1Raw.cdb_byte1(), 0x08);
        assert_eq!(SectorReadFormat::Mode2Raw.cdb_byte1(), 0x00);
    }

    #[test]
    fn main_channel_fields_are_encoded_in_cdb_byte9() {
        assert_eq!(SectorReadFormat::Audio.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode1Cooked.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode1Raw.cdb_byte9(), 0xF8);
        assert_eq!(SectorReadFormat::Mode2Raw.cdb_byte9(), 0xF8);
    }

    #[test]
    fn sector_sizes_match_mmc_layouts() {
        assert_eq!(SectorReadFormat::Audio.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Mode1Cooked.sector_size(), 2048);
        assert_eq!(SectorReadFormat::Mode1Raw.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Mode2Raw.sector_size(), 2352);
    }

    #[test]
    fn transfer_caps_stay_within_64_kib() {
        for format in [
            SectorReadFormat::Audio,
            SectorReadFormat::Mode1Cooked,
            SectorReadFormat::Mode1Raw,
            SectorReadFormat::Mode2Raw,
        ] {
            let bytes = format.max_sectors_per_xfer() as usize * format.sector_size();
            assert!(bytes <= 64 * 1024);
        }
    }
}
