use std::io;

#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub(crate) const TRACK_INFORMATION_RESPONSE_SIZE: usize = 36;

/// Track-level metadata returned by MMC READ TRACK INFORMATION (0x52).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackInformation {
    pub(crate) track_number: u16,
    pub(crate) session_number: u16,
    pub(crate) track_mode: u8,
    pub(crate) data_mode: u8,
    pub(crate) start_lba: u32,
    pub(crate) track_size: u32,
}

/// Build READ TRACK INFORMATION with the address interpreted as a track number.
#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub(crate) fn build_read_track_information_cdb(
    track_number: u8,
    allocation_length: u16,
) -> [u8; 10] {
    let mut cdb = [0u8; 10];
    cdb[0] = 0x52;
    cdb[1] = 0x01; // Address Type: track number
    cdb[5] = track_number;
    cdb[7..9].copy_from_slice(&allocation_length.to_be_bytes());
    cdb
}

/// Parse the standard MMC READ TRACK INFORMATION response.
pub(crate) fn parse_track_information(data: &[u8]) -> io::Result<TrackInformation> {
    const MINIMUM_RESPONSE_SIZE: usize = 32;

    if data.len() < MINIMUM_RESPONSE_SIZE {
        return Err(invalid_data("track information response is too short"));
    }

    let declared_size = u16::from_be_bytes([data[0], data[1]]) as usize + 2;
    if declared_size < MINIMUM_RESPONSE_SIZE || declared_size > data.len() {
        return Err(invalid_data("track information response length is invalid"));
    }

    let track_number_msb = data.get(32).copied().unwrap_or(0);
    let session_number_msb = data.get(33).copied().unwrap_or(0);

    Ok(TrackInformation {
        track_number: u16::from_be_bytes([track_number_msb, data[2]]),
        session_number: u16::from_be_bytes([session_number_msb, data[3]]),
        track_mode: data[5] & 0x0F,
        data_mode: data[6] & 0x0F,
        start_lba: read_u32(data, 8),
        track_size: read_u32(data, 24),
    })
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_track_number_request() {
        assert_eq!(
            build_read_track_information_cdb(16, TRACK_INFORMATION_RESPONSE_SIZE as u16),
            [0x52, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x24, 0x00]
        );
    }

    #[test]
    fn parses_track_information() {
        let mut data = [0u8; TRACK_INFORMATION_RESPONSE_SIZE];
        data[0..2].copy_from_slice(&34u16.to_be_bytes());
        data[2] = 16;
        data[3] = 2;
        data[5] = 0xA4;
        data[6] = 0xB1;
        data[8..12].copy_from_slice(&263_053u32.to_be_bytes());
        data[24..28].copy_from_slice(&64_655u32.to_be_bytes());

        assert_eq!(
            parse_track_information(&data).unwrap(),
            TrackInformation {
                track_number: 16,
                session_number: 2,
                track_mode: 4,
                data_mode: 1,
                start_lba: 263_053,
                track_size: 64_655,
            }
        );
    }

    #[test]
    fn parses_mmc6_extended_track_and_session_numbers() {
        let mut data = [0u8; TRACK_INFORMATION_RESPONSE_SIZE];
        data[0..2].copy_from_slice(&34u16.to_be_bytes());
        data[2] = 0x34;
        data[3] = 0x78;
        data[32] = 0x12;
        data[33] = 0x56;

        let information = parse_track_information(&data).unwrap();
        assert_eq!(information.track_number, 0x1234);
        assert_eq!(information.session_number, 0x5678);
    }

    #[test]
    fn rejects_short_or_truncated_responses() {
        assert!(parse_track_information(&[0; 31]).is_err());

        let mut truncated = [0u8; 32];
        truncated[0..2].copy_from_slice(&34u16.to_be_bytes());
        assert!(parse_track_information(&truncated).is_err());
    }
}
