/// Selects the type and layout of sectors returned when reading from an optical drive.
///
/// This is the crate's platform-independent representation of the sector type and
/// main-channel fields requested by the MMC `READ CD` command (`0xBE`) or the
/// equivalent platform API.
///
/// [`ReadOptions`](crate::ReadOptions) defaults to [`Audio`](Self::Audio), so
/// callers reading audio tracks normally do not need to select a format. For a
/// data track, call [`CdReader::detect_track_format`] and pass the result to
/// [`ReadOptions::with_format`](crate::ReadOptions::with_format). Detection
/// chooses [`Mode1Cooked`](Self::Mode1Cooked) for Mode 1 tracks and
/// [`Mode2Raw`](Self::Mode2Raw) for Mode 2 tracks.
///
/// Selecting a format does not convert the sectors. It tells the drive what
/// sector type and fields to return, so the selection must match the track being
/// read. A mismatched format may be rejected by the library or the drive.
///
/// The `Raw` variants return the complete 2,352-byte main-channel sector. They
/// do not include subchannel data or C2 error information. Use
/// [`sector_size`](Self::sector_size) to obtain the number of bytes returned per
/// sector for any variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorReadFormat {
    /// CD-DA audio as 2,352 bytes of headerless PCM per sector.
    ///
    /// The samples are signed 16-bit little-endian stereo at 44.1 kHz. Each
    /// sector contains 588 stereo sample frames and represents 1/75 second of
    /// audio. The returned bytes can be passed directly to [`create_wav`](crate::create_wav).
    Audio,

    /// The 2,048-byte user-data field from a Mode 1 sector.
    ///
    /// The drive omits the sync pattern, sector header, Error Detection Code
    /// (EDC), reserved bytes, and Error Correction Code (ECC). This is usually
    /// the preferred representation for reading filesystems; concatenating the
    /// cooked sectors of a typical ISO 9660 track produces a directly usable
    /// disc image.
    Mode1Cooked,

    /// A complete 2,352-byte Mode 1 main-channel sector.
    ///
    /// This includes the 12-byte sync pattern, 4-byte header, 2,048-byte user
    /// data field, EDC, reserved bytes, and ECC. Use this when preserving or
    /// inspecting the original sector framing. For normal filesystem access,
    /// [`Mode1Cooked`](Self::Mode1Cooked) is usually more convenient.
    Mode1Raw,

    /// A complete 2,352-byte Mode 2 main-channel sector.
    ///
    /// This is the only Mode 2 representation provided by the crate. Mode 2 XA
    /// tracks can mix Form 1 and Form 2 sectors within the same track: Form 1
    /// carries 2,048 bytes of user data with stronger error correction, while
    /// Form 2 carries 2,324 bytes of user data.
    ///
    /// The form is recorded in each sector's XA subheader. The crate does not
    /// expose a cooked Mode 2 reader or a public XA payload parser, so callers
    /// must inspect each sector and extract the appropriate payload themselves.
    Mode2Raw,
}

impl SectorReadFormat {
    pub(crate) fn is_audio(&self) -> bool {
        matches!(self, Self::Audio)
    }

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
