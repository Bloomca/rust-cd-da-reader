use super::device::Drive;
use crate::CdReaderError;
use crate::data_reader::ReadSpeed;

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
        multiplier * 176400 / 1000
    };
    let response =
        unsafe { super::ffi::request_cd_read_speed(drive.fd(), target_speed_kbs as u16) };

    if !response {
        return Err(CdReaderError::Io(std::io::Error::last_os_error()));
    }

    Ok(())
}
