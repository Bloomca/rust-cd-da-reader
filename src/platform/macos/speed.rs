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
        multiplier * 176400 / 1000
    };
    if target_speed_kbs > u16::MAX.into() {
        // TODO: Implement error handle (CdReaderError?)
        todo!();
    }
    unsafe { super::ffi::request_cd_read_speed(drive.fd(), target_speed_kbs as u16) };
    Ok(())
}
