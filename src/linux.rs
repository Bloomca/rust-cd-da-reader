use libc::{O_NONBLOCK, O_RDWR, c_uchar, c_void};
use std::cmp::min;
use std::ffi::CString;
use std::fs::File;
use std::io::Error;
use std::os::fd::{AsRawFd, FromRawFd};

use crate::Toc;
use crate::parse_toc::parse_toc;
use crate::utils::get_track_bounds;
use crate::{CdReaderError, ScsiError, ScsiOp};

const SG_INFO_CHECK: u32 = 0x1;
const SG_DXFER_FROM_DEV: i32 = -3;

// see more info here: https://tldp.org/HOWTO/SCSI-Generic-HOWTO/sg_io_hdr_t.html
#[repr(C)]
struct SgIoHeader {
    interface_id: i32,    // 'S' for SCSI
    dxfer_direction: i32, // SG_DXFER_*
    cmd_len: u8,          // CDB length -- 10 for TOC, 12 for reading data
    mx_sb_len: u8,        // max sense size to return
    iovec_count: u16,
    dxfer_len: u32,      // bytes to transfer
    dxferp: *mut c_void, // data buffer
    cmdp: *mut c_uchar,  // CDB
    sbp: *mut c_uchar,   // sense
    timeout: u32,        // ms
    flags: u32,
    pack_id: i32,
    usr_ptr: *mut c_void,
    status: u8, // SCSI status
    masked_status: u8,
    msg_status: u8,
    sb_len_wr: u8, // sense bytes actually written
    host_status: u16,
    driver_status: u16,
    resid: i32,
    duration: u32, // ms
    info: u32,
}

// _IOWR('S', 0x85, struct sg_io_hdr)
const SG_IO: u64 = 0x2285;

static mut DRIVE_HANDLE: Option<File> = None;

pub fn open_drive(path: &str) -> std::io::Result<()> {
    let c = CString::new(path).unwrap();
    let fd = unsafe { libc::open(c.as_ptr(), O_RDWR | O_NONBLOCK) };
    if fd < 0 {
        return Err(Error::last_os_error());
    }
    let drive_handle = unsafe { File::from_raw_fd(fd) };

    unsafe {
        DRIVE_HANDLE = Some(drive_handle);
    }

    Ok(())
}

#[allow(static_mut_refs)]
pub fn close_drive() {
    unsafe {
        if let Some(current_drive) = DRIVE_HANDLE.take() {
            drop(current_drive);
            DRIVE_HANDLE = None;
        }
    }
}

#[allow(static_mut_refs)]
unsafe fn ioctl_sg_io(hdr: &mut SgIoHeader) -> std::io::Result<()> {
    let fd = unsafe {
        DRIVE_HANDLE
            .as_ref()
            .ok_or_else(|| Error::new(std::io::ErrorKind::NotFound, "Drive not opened"))?
            .as_raw_fd()
    };

    let ret = unsafe { libc::ioctl(fd, SG_IO, hdr as *mut _) };
    if ret < 0 {
        return Err(Error::last_os_error());
    }

    Ok(())
}

pub fn read_toc() -> std::result::Result<Toc, CdReaderError> {
    let alloc_len: usize = 2048;
    let mut data = vec![0u8; alloc_len];
    let mut sense = vec![0u8; 32];

    let mut cdb = [0u8; 10];
    cdb[0] = 0x43;
    cdb[1] = 0; // use LBA format
    cdb[2] = 0; // get TOC
    cdb[6] = 0; // starting track
    cdb[7] = ((alloc_len >> 8) & 0xFF) as u8;
    cdb[8] = (alloc_len & 0xFF) as u8;

    let mut hdr = SgIoHeader {
        interface_id: 'S' as i32,
        dxfer_direction: SG_DXFER_FROM_DEV,
        cmd_len: cdb.len() as u8,
        mx_sb_len: sense.len() as u8,
        iovec_count: 0,
        dxfer_len: data.len() as u32,
        dxferp: data.as_mut_ptr() as *mut c_void,
        cmdp: cdb.as_mut_ptr(),
        sbp: sense.as_mut_ptr(),
        timeout: 10_000, // ms
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

    unsafe { ioctl_sg_io(&mut hdr).map_err(CdReaderError::Io)? };

    // Check if the ioctl itself succeeded
    if hdr.info & SG_INFO_CHECK != 0 {
        let (sense_key, asc, ascq) = parse_sense(&sense, hdr.sb_len_wr);
        return Err(CdReaderError::Scsi(ScsiError {
            op: ScsiOp::ReadToc,
            lba: None,
            sectors: None,
            scsi_status: hdr.status,
            sense_key,
            asc,
            ascq,
        }));
    }

    // Check SCSI status
    if hdr.status != 0 {
        let (sense_key, asc, ascq) = parse_sense(&sense, hdr.sb_len_wr);
        return Err(CdReaderError::Scsi(ScsiError {
            op: ScsiOp::ReadToc,
            lba: None,
            sectors: None,
            scsi_status: hdr.status,
            sense_key,
            asc,
            ascq,
        }));
    }

    // Trim actual length if driver reported residual
    if hdr.resid > 0 {
        let got = data.len() as i32 - hdr.resid;
        if got > 0 {
            data.truncate(got as usize);
        }
    }

    parse_toc(data).map_err(|err| CdReaderError::Parse(err.to_string()))
}

pub fn read_track(toc: &Toc, track_no: u8) -> std::result::Result<Vec<u8>, CdReaderError> {
    let (start_lba, sectors) = get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
    read_cd_audio_range(start_lba, sectors)
}

// --- READ CD (0xBE): read an arbitrary LBA range as CD-DA (2352 bytes/sector) ---
fn read_cd_audio_range(
    start_lba: u32,
    sectors: u32,
) -> std::result::Result<Vec<u8>, CdReaderError> {
    // SCSI-2 defines reading data in 2352 bytes chunks
    const SECTOR_BYTES: usize = 2352;

    // read ~64 KBs per request
    const MAX_SECTORS_PER_XFER: u32 = 27; // 27 * 2352 = 63,504 bytes

    let total_bytes = (sectors as usize) * SECTOR_BYTES;
    // allocate the entire necessary size from the beginning to avoid memory realloc
    let mut out = Vec::<u8>::with_capacity(total_bytes);

    let mut remaining = sectors;
    let mut lba = start_lba;

    while remaining > 0 {
        let this_sectors = min(remaining, MAX_SECTORS_PER_XFER);
        let mut chunk = vec![0u8; (this_sectors as usize) * SECTOR_BYTES];

        let mut sense = vec![0u8; 64];

        // CDB: READ CD (0xBE), LBA addressing
        let mut cdb = [0u8; 12];
        // fill with zeroes everywhere
        cdb.fill(0);
        cdb[0] = 0xBE; // READ CD
        cdb[2] = ((lba >> 24) & 0xFF) as u8;
        cdb[3] = ((lba >> 16) & 0xFF) as u8;
        cdb[4] = ((lba >> 8) & 0xFF) as u8;
        cdb[5] = (lba & 0xFF) as u8;
        // Transfer length in sectors (24-bit, MSB..LSB)
        cdb[6] = ((this_sectors >> 16) & 0xFF) as u8;
        cdb[7] = ((this_sectors >> 8) & 0xFF) as u8;
        cdb[8] = (this_sectors & 0xFF) as u8;
        // cdb[9] sub-channel selection flags:
        // Request only "User Data" -> 2352 bytes/sector for audio
        cdb[9] = 0x10;
        cdb[10] = 0x00; // Control
        cdb[11] = 0x00;

        let mut hdr = SgIoHeader {
            interface_id: 'S' as i32,
            dxfer_direction: SG_DXFER_FROM_DEV,
            cmd_len: cdb.len() as u8,
            mx_sb_len: sense.len() as u8,
            iovec_count: 0,
            dxfer_len: chunk.len() as u32,
            dxferp: chunk.as_mut_ptr() as *mut c_void,
            cmdp: cdb.as_mut_ptr(),
            sbp: sense.as_mut_ptr(),
            timeout: 30_000, // ms
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

        unsafe { ioctl_sg_io(&mut hdr).map_err(CdReaderError::Io)? };

        if hdr.info & SG_INFO_CHECK != 0 {
            let (sense_key, asc, ascq) = parse_sense(&sense, hdr.sb_len_wr);
            return Err(CdReaderError::Scsi(ScsiError {
                op: ScsiOp::ReadCd,
                lba: Some(lba),
                sectors: Some(this_sectors),
                scsi_status: hdr.status,
                sense_key,
                asc,
                ascq,
            }));
        }

        if hdr.status != 0 {
            let (sense_key, asc, ascq) = parse_sense(&sense, hdr.sb_len_wr);
            return Err(CdReaderError::Scsi(ScsiError {
                op: ScsiOp::ReadCd,
                lba: Some(lba),
                sectors: Some(this_sectors),
                scsi_status: hdr.status,
                sense_key,
                asc,
                ascq,
            }));
        }

        if hdr.resid > 0 {
            let got = (chunk.len() as i32 - hdr.resid).max(0) as usize;
            chunk.truncate(got);
        }

        out.extend_from_slice(&chunk);

        lba += this_sectors;
        remaining -= this_sectors;
    }

    Ok(out)
}

fn parse_sense(sense: &[u8], sb_len_wr: u8) -> (Option<u8>, Option<u8>, Option<u8>) {
    if sb_len_wr == 0 || sense.len() < 14 {
        return (None, None, None);
    }

    let sense_key = Some(sense[2] & 0x0F);
    let asc = Some(sense[12]);
    let ascq = Some(sense[13]);
    (sense_key, asc, ascq)
}
