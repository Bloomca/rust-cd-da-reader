use std::ffi::{CStr, CString};
use std::io;
use std::{ptr, slice};

use crate::data_reader::SectorReadMode;
use crate::parse_toc::parse_toc;
use crate::{CdReaderError, DriveInfo, ScsiError, ScsiOp, Toc};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct MacScsiError {
    has_scsi_error: u8,
    scsi_status: u8,
    has_sense: u8,
    sense_key: u8,
    asc: u8,
    ascq: u8,
    exec_error: u32,
    task_status: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MacDriveInfo {
    bsd_name: [libc::c_char; 64],
    has_toc: u8,
    has_audio: u8,
}

#[link(name = "macos_cd_shim", kind = "static")]
unsafe extern "C" {
    fn cd_read_toc(out_buf: *mut *mut u8, out_len: *mut u32, out_err: *mut MacScsiError) -> bool;
    fn read_cd_sectors(
        lba: u32,
        sectors: u32,
        mode_id: u32,
        out_buf: *mut *mut u8,
        out_len: *mut u32,
        out_err: *mut MacScsiError,
    ) -> bool;
    fn cd_free(p: *mut libc::c_void);
    fn list_cd_drives(out_drives: *mut *mut MacDriveInfo, out_count: *mut u32) -> bool;
    fn open_dev_session(bsd_name: *const libc::c_char) -> bool;
    fn close_dev_session();
}

pub fn list_drives() -> io::Result<Vec<DriveInfo>> {
    let mut raw_drives: *mut MacDriveInfo = ptr::null_mut();
    let mut count: u32 = 0;

    let ok = unsafe { list_cd_drives(&mut raw_drives, &mut count) };
    if !ok {
        return Err(io::Error::other("could not enumerate CD drives"));
    }

    let drives = if raw_drives.is_null() || count == 0 {
        Vec::new()
    } else {
        let raw = unsafe { slice::from_raw_parts(raw_drives, count as usize) };
        let mut drives = Vec::with_capacity(raw.len());

        for drive in raw {
            let path = unsafe { CStr::from_ptr(drive.bsd_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            if path.is_empty() {
                continue;
            }

            drives.push(DriveInfo {
                display_name: Some(path.clone()),
                path,
                has_audio_cd: drive.has_audio != 0,
            });
        }

        drives.sort_by(|a, b| a.path.cmp(&b.path));
        drives.dedup_by(|a, b| a.path == b.path);
        drives
    };

    unsafe { cd_free(raw_drives as *mut _) };

    Ok(drives)
}

pub fn open_drive(path: &str) -> std::io::Result<()> {
    let bsd = CString::new(path).unwrap();
    let result = unsafe { open_dev_session(bsd.as_ptr()) };

    if !result {
        return Err(std::io::Error::other("could not get device"));
    }

    Ok(())
}

pub fn close_drive() {
    unsafe { close_dev_session() };
}

pub fn read_toc() -> Result<Toc, CdReaderError> {
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;
    let mut err: MacScsiError = Default::default();

    let ok = unsafe { cd_read_toc(&mut buf, &mut len, &mut err) };
    if !ok {
        return Err(map_mac_error(err, ScsiOp::ReadToc, None, None));
    }
    let data = unsafe { slice::from_raw_parts(buf, len as usize) };

    // `.to_vec()` will copy the data, so we can free it safely after
    let result =
        parse_toc(data.to_vec()).map_err(|parse_err| CdReaderError::Parse(parse_err.to_string()));

    unsafe { cd_free(buf as *mut _) };

    result
}

pub(crate) fn read_cd_chunk(
    lba: u32,
    sectors: u32,
    mode: SectorReadMode,
) -> Result<Vec<u8>, CdReaderError> {
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;
    let mut err: MacScsiError = Default::default();

    // Discriminant understood by `read_cd_sectors`, which maps it to the
    // matching macOS CD sector area/type for DKIOCCDREAD.
    let mode_id: u32 = match mode {
        SectorReadMode::Audio => 0,
        SectorReadMode::DataCooked => 1,
        SectorReadMode::DataRaw => 2,
    };

    let ok = unsafe { read_cd_sectors(lba, sectors, mode_id, &mut buf, &mut len, &mut err) };

    if !ok {
        return Err(map_mac_error(err, ScsiOp::ReadCd, Some(lba), Some(sectors)));
    }

    let data = unsafe { slice::from_raw_parts(buf, len as usize) };
    let result = data.to_vec();
    unsafe { cd_free(buf as *mut _) };

    Ok(result)
}

fn map_mac_error(
    err: MacScsiError,
    op: ScsiOp,
    lba: Option<u32>,
    sectors: Option<u32>,
) -> CdReaderError {
    if err.has_scsi_error != 0 {
        return CdReaderError::Scsi(ScsiError {
            op,
            lba,
            sectors,
            scsi_status: err.scsi_status,
            sense_key: (err.has_sense != 0).then_some(err.sense_key),
            asc: (err.has_sense != 0).then_some(err.asc),
            ascq: (err.has_sense != 0).then_some(err.ascq),
        });
    }

    CdReaderError::Io(std::io::Error::other("macOS CD command failed"))
}
