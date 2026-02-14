use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::Storage::IscsiDisc::{
    IOCTL_SCSI_PASS_THROUGH_DIRECT, SCSI_IOCTL_DATA_IN, SCSI_PASS_THROUGH_DIRECT,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::{CdReaderError, RetryConfig, ScsiError, ScsiOp, Toc, parse_toc, windows_read_track};

use std::mem;
use std::ptr;

#[repr(C)]
pub struct SptdWithSense {
    pub sptd: SCSI_PASS_THROUGH_DIRECT,
    pub sense: [u8; 32],
}

static mut DRIVE_HANDLE: Option<HANDLE> = None;

#[allow(static_mut_refs)]
pub fn open_drive(path: &str) -> std::io::Result<()> {
    unsafe {
        if DRIVE_HANDLE.is_some() {
            return Err(std::io::Error::other("Drive is already open"));
        }
    }

    let path_wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    let drive_handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };

    if drive_handle == INVALID_HANDLE_VALUE {
        println!("Device NOT opened succesfully");
        return Err(std::io::Error::last_os_error());
    }

    unsafe {
        DRIVE_HANDLE = Some(drive_handle);
    }

    Ok(())
}

pub fn close_drive() {
    unsafe {
        if let Some(current_drive) = DRIVE_HANDLE {
            CloseHandle(current_drive);
            DRIVE_HANDLE = None;
        }
    }
}

pub fn read_toc() -> Result<Toc, CdReaderError> {
    let handle = unsafe {
        DRIVE_HANDLE
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Drive not opened"))
            .map_err(CdReaderError::Io)?
    };

    // Buffer that the device will fill with TOC data.
    // 4 + (99 * 8) = 796 max-ish for format 0x00; 2 KiB is safe.
    let alloc_len: usize = 2048;
    let mut data = vec![0u8; alloc_len];

    let mut wrapper: SptdWithSense = unsafe { mem::zeroed() };
    let sptd = &mut wrapper.sptd;

    sptd.Length = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u16;
    sptd.CdbLength = 10; // READ TOC is a 10-byte CDB
    sptd.DataIn = SCSI_IOCTL_DATA_IN as u8;
    sptd.TimeOutValue = 10; // 10 seconds
    sptd.DataTransferLength = alloc_len as u32;
    sptd.DataBuffer = data.as_mut_ptr() as *mut _;

    // Sense buffer immediately follows the struct
    sptd.SenseInfoLength = wrapper.sense.len() as u8;
    // the offset is equal to the first property
    sptd.SenseInfoOffset = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u32;

    // Build CDB for READ TOC/PMA/ATIP (0x43), Format = 0x00 (TOC), MSF = 1
    // CDB layout (10B):
    // [0]=0x43, [1]=MSF bit in bit1, [2]=Format, [6]=StartingTrack,
    // [7..8]=AllocationLength (MSB..LSB), [9]=Control
    let cdb = &mut sptd.Cdb;
    cdb[0] = 0x43; // READ TOC/PMA/ATIP
    cdb[1] = 0x00; // LBA format
    cdb[2] = 0x00; // Format 0x00: TOC
    cdb[6] = 0x00; // Starting track 0 = first track/session
    cdb[7] = ((alloc_len >> 8) & 0xFF) as u8;
    cdb[8] = (alloc_len & 0xFF) as u8;
    cdb[9] = 0x00;

    let mut bytes = 0u32;

    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_SCSI_PASS_THROUGH_DIRECT,
            &mut wrapper as *mut _ as *mut _,
            size_of::<SptdWithSense>() as u32,
            &mut wrapper as *mut _ as *mut _,
            size_of::<SptdWithSense>() as u32,
            &mut bytes as *mut u32,
            ptr::null_mut(),
        )
    };

    if ok == 0 {
        return Err(CdReaderError::Io(std::io::Error::last_os_error()));
    } else if wrapper.sptd.ScsiStatus != 0 {
        let (sense_key, asc, ascq) = parse_sense(&wrapper.sense, wrapper.sptd.SenseInfoLength);
        return Err(CdReaderError::Scsi(ScsiError {
            op: ScsiOp::ReadToc,
            lba: None,
            sectors: None,
            scsi_status: wrapper.sptd.ScsiStatus,
            sense_key,
            asc,
            ascq,
        }));
    }

    parse_toc::parse_toc(data).map_err(|err| CdReaderError::Parse(err.to_string()))
}

pub fn read_track(toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdReaderError> {
    read_track_with_retry(toc, track_no, &RetryConfig::default())
}

pub fn read_track_with_retry(
    toc: &Toc,
    track_no: u8,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    let handle = unsafe {
        DRIVE_HANDLE
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Drive not opened"))
            .map_err(CdReaderError::Io)?
    };
    windows_read_track::read_track_with_retry(handle, toc, track_no, cfg)
}

fn parse_sense(sense: &[u8], sense_len: u8) -> (Option<u8>, Option<u8>, Option<u8>) {
    if sense_len < 14 || sense.len() < 14 {
        return (None, None, None);
    }

    (Some(sense[2] & 0x0F), Some(sense[12]), Some(sense[13]))
}
