use super::device::Drive;
use crate::CdReaderError;
use crate::data_reader::ReadSpeed;

pub(super) fn request_read_speed(
    drive: &Drive,
    target_read_speed: ReadSpeed,
) -> Result<(), CdReaderError> {
    // stub
    /*
    let multiplier = match target_read_speed {
        ReadSpeed::Unchanged => return Ok(()),
        ReadSpeed::Optimal => 0,
        ReadSpeed::CustomMultiplier(x) => x,
    };
    */
    Ok(())
}
