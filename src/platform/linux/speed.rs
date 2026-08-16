use super::device::Drive;
use crate::CdReaderError;
use crate::data_reader::ReadSpeed;
use std::os::fd::RawFd;

// From linux/include/uapi/linux/cdrom.h
const CDROM_SELECT_SPEED: libc::c_ulong = 0x5322;

pub(super) fn request_read_speed(
    drive: &Drive,
    target_read_speed: ReadSpeed,
) -> Result<(), CdReaderError> {
    let multiplier = match target_read_speed {
        ReadSpeed::Unchanged => return Ok(()),
        ReadSpeed::Optimal => 0,
        ReadSpeed::CustomMultiplier(x) => x,
    };

    execute_request_read_speed(drive.fd(), multiplier)
}

fn execute_request_read_speed(fd: RawFd, multiplier: u8) -> Result<(), CdReaderError> {
    let result = unsafe { libc::ioctl(fd, CDROM_SELECT_SPEED, multiplier as libc::c_ulong) };
    if result < 0 {
        Err(CdReaderError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}
