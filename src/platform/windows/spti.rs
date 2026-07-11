use std::mem::{offset_of, size_of};
use std::ptr;

use windows_sys::Win32::Storage::IscsiDisc::{
    IOCTL_SCSI_PASS_THROUGH_DIRECT, SCSI_IOCTL_DATA_IN, SCSI_PASS_THROUGH_DIRECT,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use super::device;
use crate::{CdReaderError, ScsiError, ScsiOp};

const SENSE_BUFFER_SIZE: usize = 32;

#[repr(C)]
struct SptdWithSense {
    sptd: SCSI_PASS_THROUGH_DIRECT,
    sense: [u8; SENSE_BUFFER_SIZE],
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CommandContext {
    pub(super) op: ScsiOp,
    pub(super) lba: Option<u32>,
    pub(super) sectors: Option<u32>,
}

/// Execute one SCSI read through Windows SPTI and return the transferred byte count.
pub(super) fn execute_read(
    cdb: &[u8],
    output: &mut [u8],
    timeout_seconds: u32,
    context: CommandContext,
) -> Result<usize, CdReaderError> {
    if cdb.len() > 16 {
        return Err(invalid_input("SCSI CDB exceeds the Windows 16-byte limit"));
    }

    let transfer_len = u32::try_from(output.len())
        .map_err(|_| invalid_input("SCSI transfer buffer is too large"))?;
    let handle = device::drive_handle().map_err(CdReaderError::Io)?;
    let mut wrapper: SptdWithSense = unsafe { std::mem::zeroed() };

    wrapper.sptd.Length = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u16;
    wrapper.sptd.CdbLength = cdb.len() as u8;
    wrapper.sptd.DataIn = SCSI_IOCTL_DATA_IN as u8;
    wrapper.sptd.TimeOutValue = timeout_seconds;
    wrapper.sptd.DataTransferLength = transfer_len;
    wrapper.sptd.DataBuffer = output.as_mut_ptr().cast();
    wrapper.sptd.SenseInfoLength = wrapper.sense.len() as u8;
    wrapper.sptd.SenseInfoOffset = offset_of!(SptdWithSense, sense) as u32;
    wrapper.sptd.Cdb[..cdb.len()].copy_from_slice(cdb);

    let mut bytes_returned = 0u32;
    let result = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_SCSI_PASS_THROUGH_DIRECT,
            &mut wrapper as *mut _ as *mut _,
            size_of::<SptdWithSense>() as u32,
            &mut wrapper as *mut _ as *mut _,
            size_of::<SptdWithSense>() as u32,
            &mut bytes_returned,
            ptr::null_mut(),
        )
    };

    if result == 0 {
        return Err(CdReaderError::Io(std::io::Error::last_os_error()));
    }

    if wrapper.sptd.ScsiStatus != 0 {
        let (sense_key, asc, ascq) = parse_sense(&wrapper.sense, wrapper.sptd.SenseInfoLength);
        return Err(CdReaderError::Scsi(ScsiError {
            op: context.op,
            lba: context.lba,
            sectors: context.sectors,
            scsi_status: wrapper.sptd.ScsiStatus,
            sense_key,
            asc,
            ascq,
        }));
    }

    Ok((wrapper.sptd.DataTransferLength as usize).min(output.len()))
}

fn invalid_input(message: &'static str) -> CdReaderError {
    CdReaderError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message,
    ))
}

fn parse_sense(sense: &[u8], written: u8) -> (Option<u8>, Option<u8>, Option<u8>) {
    if written < 14 || sense.len() < 14 {
        return (None, None, None);
    }

    (Some(sense[2] & 0x0F), Some(sense[12]), Some(sense[13]))
}
