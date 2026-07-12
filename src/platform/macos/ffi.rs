use crate::{CdReaderError, ScsiError, ScsiOp};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct MacScsiError {
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
pub(super) struct MacDriveInfo {
    pub(super) bsd_name: [libc::c_char; 64],
    pub(super) has_toc: u8,
    pub(super) has_audio: u8,
}

#[link(name = "macos_cd_shim", kind = "static")]
unsafe extern "C" {
    pub(super) fn cd_read_toc(
        fd: libc::c_int,
        out_buf: *mut *mut u8,
        out_len: *mut u32,
        out_err: *mut MacScsiError,
    ) -> bool;
    pub(super) fn read_cd_sectors(
        fd: libc::c_int,
        lba: u32,
        sectors: u32,
        mode_id: u32,
        out_buf: *mut *mut u8,
        out_len: *mut u32,
        out_err: *mut MacScsiError,
    ) -> bool;
    pub(super) fn cd_free(pointer: *mut libc::c_void);
    pub(super) fn list_cd_drives(out_drives: *mut *mut MacDriveInfo, out_count: *mut u32) -> bool;
    pub(super) fn open_cd_raw_device(bsd_name: *const libc::c_char) -> libc::c_int;
}

pub(super) fn map_error(
    error: MacScsiError,
    op: ScsiOp,
    lba: Option<u32>,
    sectors: Option<u32>,
) -> CdReaderError {
    if error.has_scsi_error != 0 {
        return CdReaderError::Scsi(ScsiError {
            op,
            lba,
            sectors,
            scsi_status: error.scsi_status,
            sense_key: (error.has_sense != 0).then_some(error.sense_key),
            asc: (error.has_sense != 0).then_some(error.asc),
            ascq: (error.has_sense != 0).then_some(error.ascq),
        });
    }

    CdReaderError::Io(std::io::Error::other("macOS CD command failed"))
}
