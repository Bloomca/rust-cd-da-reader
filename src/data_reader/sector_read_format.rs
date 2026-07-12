/// Sector format requested through the READ CD (0xBE) command.
///
/// Each variant selects an expected sector type through CDB byte 1 and the
/// returned main-channel fields through CDB byte 9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorReadFormat {
    /// Complete 2352-byte main-channel sector without declaring an expected
    /// sector type.
    ///
    /// This is primarily useful for sector-type detection. Some drives may not
    /// support raw reads with an expected type of “Any.”
    AnyRaw,

    /// CD-DA audio: 2352 bytes of PCM per sector.
    Audio,

    /// Mode 1 user data only: 2048 bytes per sector.
    Mode1Cooked,
    /// Complete Mode 1 sector: 2352 bytes with sync, header, user data, EDC,
    /// and ECC.
    Mode1Raw,

    /// Mode 2 formless user data: 2336 bytes per sector.
    Mode2FormlessCooked,
    /// Complete Mode 2 formless sector: 2352 bytes.
    Mode2FormlessRaw,

    /// Mode 2 Form 1 user data: 2048 bytes per sector.
    Mode2Form1Cooked,
    /// Complete Mode 2 Form 1 sector: 2352 bytes.
    Mode2Form1Raw,

    /// Mode 2 Form 2 user area: 2328 bytes per sector.
    ///
    /// This consists of 2324 bytes of application payload followed by its
    /// 4-byte EDC, as exposed by MMC and macOS IOKit.
    Mode2Form2Cooked,
    /// Complete Mode 2 Form 2 sector: 2352 bytes.
    Mode2Form2Raw,
}

impl SectorReadFormat {
    /// Bytes returned per sector for this format.
    pub fn sector_size(&self) -> usize {
        match self {
            Self::AnyRaw
            | Self::Audio
            | Self::Mode1Raw
            | Self::Mode2FormlessRaw
            | Self::Mode2Form1Raw
            | Self::Mode2Form2Raw => 2352,
            Self::Mode1Cooked | Self::Mode2Form1Cooked => 2048,
            Self::Mode2FormlessCooked => 2336,
            Self::Mode2Form2Cooked => 2328,
        }
    }

    /// CDB byte 1: Expected Sector Type in bits 4–2.
    pub fn cdb_byte1(&self) -> u8 {
        match self {
            Self::AnyRaw => 0x00,
            Self::Audio => 0x04,
            Self::Mode1Cooked | Self::Mode1Raw => 0x08,
            Self::Mode2FormlessCooked | Self::Mode2FormlessRaw => 0x0C,
            Self::Mode2Form1Cooked | Self::Mode2Form1Raw => 0x10,
            Self::Mode2Form2Cooked | Self::Mode2Form2Raw => 0x14,
        }
    }

    /// CDB byte 9: Main Channel Selection.
    pub fn cdb_byte9(&self) -> u8 {
        match self {
            Self::Audio
            | Self::Mode1Cooked
            | Self::Mode2FormlessCooked
            | Self::Mode2Form1Cooked
            | Self::Mode2Form2Cooked => 0x10,
            Self::AnyRaw
            | Self::Mode1Raw
            | Self::Mode2FormlessRaw
            | Self::Mode2Form1Raw
            | Self::Mode2Form2Raw => 0xF8,
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
        assert_eq!(SectorReadFormat::AnyRaw.cdb_byte1(), 0x00);
        assert_eq!(SectorReadFormat::Audio.cdb_byte1(), 0x04);
        assert_eq!(SectorReadFormat::Mode1Cooked.cdb_byte1(), 0x08);
        assert_eq!(SectorReadFormat::Mode1Raw.cdb_byte1(), 0x08);
        assert_eq!(SectorReadFormat::Mode2FormlessCooked.cdb_byte1(), 0x0C);
        assert_eq!(SectorReadFormat::Mode2FormlessRaw.cdb_byte1(), 0x0C);
        assert_eq!(SectorReadFormat::Mode2Form1Cooked.cdb_byte1(), 0x10);
        assert_eq!(SectorReadFormat::Mode2Form1Raw.cdb_byte1(), 0x10);
        assert_eq!(SectorReadFormat::Mode2Form2Cooked.cdb_byte1(), 0x14);
        assert_eq!(SectorReadFormat::Mode2Form2Raw.cdb_byte1(), 0x14);
    }

    #[test]
    fn main_channel_fields_are_encoded_in_cdb_byte9() {
        assert_eq!(SectorReadFormat::AnyRaw.cdb_byte9(), 0xF8);
        assert_eq!(SectorReadFormat::Audio.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode1Cooked.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode1Raw.cdb_byte9(), 0xF8);
        assert_eq!(SectorReadFormat::Mode2FormlessCooked.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode2FormlessRaw.cdb_byte9(), 0xF8);
        assert_eq!(SectorReadFormat::Mode2Form1Cooked.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode2Form1Raw.cdb_byte9(), 0xF8);
        assert_eq!(SectorReadFormat::Mode2Form2Cooked.cdb_byte9(), 0x10);
        assert_eq!(SectorReadFormat::Mode2Form2Raw.cdb_byte9(), 0xF8);
    }

    #[test]
    fn sector_sizes_match_mmc_layouts() {
        assert_eq!(SectorReadFormat::AnyRaw.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Audio.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Mode1Cooked.sector_size(), 2048);
        assert_eq!(SectorReadFormat::Mode1Raw.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Mode2FormlessCooked.sector_size(), 2336);
        assert_eq!(SectorReadFormat::Mode2FormlessRaw.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Mode2Form1Cooked.sector_size(), 2048);
        assert_eq!(SectorReadFormat::Mode2Form1Raw.sector_size(), 2352);
        assert_eq!(SectorReadFormat::Mode2Form2Cooked.sector_size(), 2328);
        assert_eq!(SectorReadFormat::Mode2Form2Raw.sector_size(), 2352);
    }

    #[test]
    fn transfer_caps_stay_within_64_kib() {
        for format in [
            SectorReadFormat::AnyRaw,
            SectorReadFormat::Audio,
            SectorReadFormat::Mode1Cooked,
            SectorReadFormat::Mode1Raw,
            SectorReadFormat::Mode2FormlessCooked,
            SectorReadFormat::Mode2FormlessRaw,
            SectorReadFormat::Mode2Form1Cooked,
            SectorReadFormat::Mode2Form1Raw,
            SectorReadFormat::Mode2Form2Cooked,
            SectorReadFormat::Mode2Form2Raw,
        ] {
            let bytes = format.max_sectors_per_xfer() as usize * format.sector_size();
            assert!(bytes <= 64 * 1024);
        }
    }
}
