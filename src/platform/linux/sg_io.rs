use libc::{c_uchar, c_void};
use std::os::fd::RawFd;

use crate::{CdReaderError, ScsiError, ScsiOp};

const SG_INFO_CHECK: u32 = 0x1;
const SG_DXFER_FROM_DEV: i32 = -3;

// _IOWR('S', 0x85, struct sg_io_hdr). Typed as `c_ulong` to match the
// `request` parameter of `libc::ioctl` on both 32-bit and 64-bit targets.
const SG_IO: libc::c_ulong = 0x2285;

// Linux's userspace representation of `sg_io_hdr_t`.
#[repr(C)]
struct SgIoHeader {
    interface_id: i32,
    dxfer_direction: i32,
    cmd_len: u8,
    mx_sb_len: u8,
    iovec_count: u16,
    dxfer_len: u32,
    dxferp: *mut c_void,
    cmdp: *mut c_uchar,
    sbp: *mut c_uchar,
    timeout: u32,
    flags: u32,
    pack_id: i32,
    usr_ptr: *mut c_void,
    status: u8,
    masked_status: u8,
    msg_status: u8,
    sb_len_wr: u8,
    host_status: u16,
    driver_status: u16,
    resid: i32,
    duration: u32,
    info: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CommandContext {
    pub(super) op: ScsiOp,
    pub(super) lba: Option<u32>,
    pub(super) sectors: Option<u32>,
}

/// Execute a single SCSI read command and return the number of bytes transferred.
pub(super) fn execute_read(
    fd: RawFd,
    cdb: &mut [u8],
    output: &mut [u8],
    timeout_ms: u32,
    context: CommandContext,
) -> Result<usize, CdReaderError> {
    let mut sense = [0u8; 64];
    let transfer_len = u32::try_from(output.len()).map_err(|_| {
        CdReaderError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "SCSI transfer buffer is too large",
        ))
    })?;

    let mut header = SgIoHeader {
        interface_id: 'S' as i32,
        dxfer_direction: SG_DXFER_FROM_DEV,
        cmd_len: cdb.len() as u8,
        mx_sb_len: sense.len() as u8,
        iovec_count: 0,
        dxfer_len: transfer_len,
        dxferp: output.as_mut_ptr().cast(),
        cmdp: cdb.as_mut_ptr(),
        sbp: sense.as_mut_ptr(),
        timeout: timeout_ms,
        flags: 0,
        pack_id: 0,
        usr_ptr: std::ptr::null_mut(),
        status: 0,
        masked_status: 0,
        msg_status: 0,
        sb_len_wr: 0,
        host_status: 0,
        driver_status: 0,
        resid: 0,
        duration: 0,
        info: 0,
    };

    let result = unsafe { libc::ioctl(fd, SG_IO, &mut header as *mut _) };
    if result < 0 {
        return Err(CdReaderError::Io(std::io::Error::last_os_error()));
    }

    if header.info & SG_INFO_CHECK != 0 || header.status != 0 {
        let (sense_key, asc, ascq) = parse_sense(&sense, header.sb_len_wr);
        return Err(CdReaderError::Scsi(ScsiError {
            op: context.op,
            lba: context.lba,
            sectors: context.sectors,
            scsi_status: header.status,
            sense_key,
            asc,
            ascq,
        }));
    }

    if header.resid <= 0 {
        return Ok(output.len());
    }

    Ok(output.len().saturating_sub(header.resid as usize))
}

fn parse_sense(sense: &[u8], written: u8) -> (Option<u8>, Option<u8>, Option<u8>) {
    if written < 14 || sense.len() < 14 {
        return (None, None, None);
    }

    (Some(sense[2] & 0x0F), Some(sense[12]), Some(sense[13]))
}
