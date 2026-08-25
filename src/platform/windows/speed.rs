use std::ptr;

use super::device::Drive;
use crate::CdReaderError;
use crate::data_reader::ReadSpeed;

use windows_sys::Win32::Devices::Cdrom::{
    CDROM_SET_SPEED, CdromDefaultRotation, CdromSetSpeed, IOCTL_CDROM_SET_SPEED,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

pub(super) fn request_read_speed(
    drive: &Drive,
    target_read_speed: ReadSpeed,
) -> Result<(), CdReaderError> {
    let multiplier = match target_read_speed {
        ReadSpeed::Unchanged => return Ok(()),
        ReadSpeed::Optimal => 0,
        ReadSpeed::CustomMultiplier(x) => x,
    };

    // Windows expects KB/s rather than an X multiplier.
    // 0xffff is the MMC drive-selected/maximum-speed sentinel.
    let target_speed_kbs = if multiplier == 0 {
        u16::MAX
    } else {
        (u32::from(multiplier) * 176_400 / 1000) as u16
    };

    let request = CDROM_SET_SPEED {
        RequestType: CdromSetSpeed,
        ReadSpeed: target_speed_kbs,
        WriteSpeed: u16::MAX,
        RotationControl: CdromDefaultRotation,
    };

    let mut bytes_returned = 0;

    let result = unsafe {
        DeviceIoControl(
            drive.handle(),
            IOCTL_CDROM_SET_SPEED,
            &request as *const _ as *const _,
            size_of::<CDROM_SET_SPEED>() as u32,
            ptr::null_mut(),
            0,
            &mut bytes_returned,
            ptr::null_mut(),
        )
    };

    if result == 0 {
        Err(CdReaderError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}
