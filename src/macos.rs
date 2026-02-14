use std::{ffi::CString, ptr, slice};

use crate::parse_toc::parse_toc;
use crate::utils::get_track_bounds;
use crate::{CdReaderError, ScsiError, ScsiOp, Toc};

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

#[link(name = "macos_cd_shim", kind = "static")]
unsafe extern "C" {
    fn start_da_guard(bsd_name: *const libc::c_char);
    fn stop_da_guard();
    fn cd_read_toc(out_buf: *mut *mut u8, out_len: *mut u32, out_err: *mut MacScsiError) -> bool;
    fn read_cd_audio(
        lba: u32,
        sectors: u32,
        out_buf: *mut *mut u8,
        out_len: *mut u32,
        out_err: *mut MacScsiError,
    ) -> bool;
    fn cd_free(p: *mut libc::c_void);
    fn get_dev_svc(bsd_name: *const libc::c_char) -> bool;
    fn reset_dev_scv();
}

pub fn open_drive(path: &str) -> std::io::Result<()> {
    let bsd = CString::new(path).unwrap();
    unsafe { start_da_guard(bsd.as_ptr()) };
    let result = unsafe { get_dev_svc(bsd.as_ptr()) };

    if !result {
        return Err(std::io::Error::other("could not get device"));
    }

    Ok(())
}

pub fn close_drive() {
    unsafe { reset_dev_scv() };
    unsafe { stop_da_guard() };
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

pub fn read_track(toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdReaderError> {
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;
    let mut err: MacScsiError = Default::default();

    let (start_lba, sectors) = get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
    let ok = unsafe { read_cd_audio(start_lba, sectors, &mut buf, &mut len, &mut err) };

    if !ok {
        return Err(map_mac_error(
            err,
            ScsiOp::ReadCd,
            Some(start_lba),
            Some(sectors),
        ));
    }

    let data = unsafe { slice::from_raw_parts(buf, len as usize) };

    // `.to_vec()` will copy the data, so we can free it safely after
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

    CdReaderError::Io(std::io::Error::other("macOS SCSI command failed"))
}
