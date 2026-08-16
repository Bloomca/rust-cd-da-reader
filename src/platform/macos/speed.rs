use super::device::Drive;
use crate::data_reader::ReadSpeed;
use crate::CdReaderError;

pub(super) fn request_read_speed(
    drive: &Drive,
    target_read_speed: ReadSpeed,
) -> Result<(), CdReaderError> {
    let multiplier = match target_read_speed {
        ReadSpeed::Unchanged => return Ok(()),
        ReadSpeed::Optimal => 0,
        ReadSpeed::CustomMultiplier(x) => x as u32,
    };
    let target_speed_kbs = if multiplier == 0 {
        0xffff
    } else {
        // `multiplier` must be less than 256, so below expression will not overflow
        // For reference, It is 0xb066 when multiplier = 256.
        multiplier * 176400 / 1000
    };
    unsafe { super::ffi::request_cd_read_speed(drive.fd(), target_speed_kbs as u16) };
    Ok(())
}
