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
    /// CDB byte 1 = 0x04 (Mode 1), byte 9 = 0x10 (user data).
    DataCooked,
    /// Data raw: 2352 bytes/sector with sync + header + user data + EDC/ECC.
    /// CDB byte 1 = 0x04 (Mode 1), byte 9 = 0xF8 (sync + header + user data + EDC/ECC).
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

    /// CDB byte 1: Expected Sector Type field (bits 5-2).
    pub fn cdb_byte1(&self) -> u8 {
        match self {
            SectorReadMode::Audio => 0x00,
            SectorReadMode::DataCooked => 0x04,
            SectorReadMode::DataRaw => 0x04,
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

    pub(crate) fn max_sectors_per_xfer(&self) -> u32 {
        match self.sector_size() {
            2048 => 32,
            _ => 27,
        }
    }
}
