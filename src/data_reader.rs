/// Sector read mode for the READ CD (0xBE) command.
///
/// Controls CDB byte 1 (Expected Sector Type) and byte 9 (Main Channel Selection)
/// to read different sector formats from the same READ CD command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorReadMode {
    /// Audio: 2352 bytes/sector, raw PCM.
    /// CDB byte 1 = 0x00 (any type), byte 9 = 0x10 (user data).
    Audio,
    /// Data cooked: 2048 bytes/sector, user data only (no sync/header/EDC/ECC).
    /// CDB byte 1 = 0x04 (Mode 1), byte 9 = 0x10 (user data).
    DataCooked,
    /// Data raw: 2352 bytes/sector with sync + header + user data + EDC/ECC.
    /// CDB byte 1 = 0x04 (Mode 1), byte 9 = 0xF8 (sync + header + user data + EDC/ECC).
    DataRaw,
}

impl SectorReadMode {
    /// Bytes per sector for this read mode.
    pub fn sector_size(&self) -> usize {
        match self {
            SectorReadMode::Audio => 2352,
            SectorReadMode::DataCooked => 2048,
            SectorReadMode::DataRaw => 2352,
        }
    }

    /// CDB byte 1: Expected Sector Type field (bits 5-2).
    pub fn cdb_byte1(&self) -> u8 {
        match self {
            SectorReadMode::Audio => 0x00,
            SectorReadMode::DataCooked => 0x04,
            SectorReadMode::DataRaw => 0x04,
        }
    }

    /// CDB byte 9: Main Channel Selection bits.
    pub fn cdb_byte9(&self) -> u8 {
        match self {
            SectorReadMode::Audio => 0x10,
            SectorReadMode::DataCooked => 0x10,
            SectorReadMode::DataRaw => 0xF8,
        }
    }
}

/// Read sectors from the disc in the specified mode with retry logic.
///
/// This parallels the existing `read_sectors_with_retry` but allows choosing
/// between audio, cooked data, and raw data sector formats.
pub fn read_data_sectors(
    lba: u32,
    sectors: u32,
    mode: SectorReadMode,
    cfg: &crate::RetryConfig,
) -> Result<Vec<u8>, crate::CdReaderError> {
    let sector_size = mode.sector_size();
    let max_sectors_per_xfer: u32 = match sector_size {
        2048 => 32, // 32 * 2048 = 65536 bytes
        _ => 27,    // 27 * 2352 = 63504 bytes
    };

    let total_bytes = (sectors as usize) * sector_size;
    let mut out = Vec::<u8>::with_capacity(total_bytes);

    let mut remaining = sectors;
    let mut cur_lba = lba;
    let attempts_total = cfg.max_attempts.max(1);

    while remaining > 0 {
        let mut chunk_sectors = remaining.min(max_sectors_per_xfer);
        let min_chunk = cfg.min_sectors_per_read.max(1);
        let mut backoff_ms = cfg.initial_backoff_ms;
        let mut last_err: Option<crate::CdReaderError> = None;

        for attempt in 1..=attempts_total {
            match read_data_chunk(cur_lba, chunk_sectors, &mode) {
                Ok(chunk) => {
                    out.extend_from_slice(&chunk);
                    cur_lba += chunk_sectors;
                    remaining -= chunk_sectors;
                    last_err = None;
                    break;
                }
                Err(err) => {
                    last_err = Some(err);
                    if attempt == attempts_total {
                        break;
                    }
                    if cfg.reduce_chunk_on_retry && chunk_sectors > min_chunk {
                        chunk_sectors = next_chunk_size(chunk_sectors, min_chunk);
                    }
                    if backoff_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                    }
                    if cfg.max_backoff_ms > 0 {
                        backoff_ms = (backoff_ms.saturating_mul(2)).min(cfg.max_backoff_ms);
                    }
                }
            }
        }

        if let Some(err) = last_err {
            return Err(err);
        }
    }

    Ok(out)
}

fn next_chunk_size(current: u32, min_chunk: u32) -> u32 {
    if current > 8 {
        8.max(min_chunk)
    } else {
        min_chunk
    }
}

// ── Platform-specific read_data_chunk implementations ──

#[cfg(target_os = "linux")]
fn read_data_chunk(
    lba: u32,
    this_sectors: u32,
    mode: &SectorReadMode,
) -> Result<Vec<u8>, crate::CdReaderError> {
    use crate::{CdReaderError, ScsiError, ScsiOp};
    use libc::{c_uchar, c_void};

    let sector_size = mode.sector_size();
    let mut chunk = vec![0u8; (this_sectors as usize) * sector_size];
    let mut sense = vec![0u8; 64];

    let mut cdb = [0u8; 12];
    cdb[0] = 0xBE; // READ CD
    cdb[1] = mode.cdb_byte1();
    cdb[2] = ((lba >> 24) & 0xFF) as u8;
    cdb[3] = ((lba >> 16) & 0xFF) as u8;
    cdb[4] = ((lba >> 8) & 0xFF) as u8;
    cdb[5] = (lba & 0xFF) as u8;
    cdb[6] = ((this_sectors >> 16) & 0xFF) as u8;
    cdb[7] = ((this_sectors >> 8) & 0xFF) as u8;
    cdb[8] = (this_sectors & 0xFF) as u8;
    cdb[9] = mode.cdb_byte9();

    let fd = crate::linux::get_drive_fd()?;

    // Build SgIoHeader inline (same layout as linux.rs)
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

    const SG_DXFER_FROM_DEV: i32 = -3;
    const SG_INFO_CHECK: u32 = 0x1;
    const SG_IO: u64 = 0x2285;

    let mut hdr = SgIoHeader {
        interface_id: b'S' as i32,
        dxfer_direction: SG_DXFER_FROM_DEV,
        cmd_len: cdb.len() as u8,
        mx_sb_len: sense.len() as u8,
        iovec_count: 0,
        dxfer_len: chunk.len() as u32,
        dxferp: chunk.as_mut_ptr() as *mut c_void,
        cmdp: cdb.as_mut_ptr(),
        sbp: sense.as_mut_ptr(),
        timeout: 30_000,
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

    let ret = unsafe { libc::ioctl(fd, SG_IO, &mut hdr as *mut _) };
    if ret < 0 {
        return Err(CdReaderError::Io(std::io::Error::last_os_error()));
    }

    if hdr.info & SG_INFO_CHECK != 0 || hdr.status != 0 {
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

    Ok(chunk)
}

#[cfg(target_os = "windows")]
fn read_data_chunk(
    lba: u32,
    this_sectors: u32,
    mode: &SectorReadMode,
) -> Result<Vec<u8>, crate::CdReaderError> {
    use crate::windows::SptdWithSense;
    use crate::{CdReaderError, ScsiError, ScsiOp};
    use windows_sys::Win32::Storage::IscsiDisc::{
        IOCTL_SCSI_PASS_THROUGH_DIRECT, SCSI_IOCTL_DATA_IN, SCSI_PASS_THROUGH_DIRECT,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    let sector_size = mode.sector_size();
    let mut chunk = vec![0u8; (this_sectors as usize) * sector_size];

    let mut wrapper: SptdWithSense = unsafe { std::mem::zeroed() };
    let sptd = &mut wrapper.sptd;

    sptd.Length = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u16;
    sptd.CdbLength = 12;
    sptd.DataIn = SCSI_IOCTL_DATA_IN as u8;
    sptd.TimeOutValue = 30;
    sptd.DataTransferLength = chunk.len() as u32;
    sptd.DataBuffer = chunk.as_mut_ptr() as *mut _;
    sptd.SenseInfoLength = wrapper.sense.len() as u8;
    sptd.SenseInfoOffset = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u32;

    let cdb = &mut sptd.Cdb;
    cdb.fill(0);
    cdb[0] = 0xBE;
    cdb[1] = mode.cdb_byte1();
    cdb[2] = ((lba >> 24) & 0xFF) as u8;
    cdb[3] = ((lba >> 16) & 0xFF) as u8;
    cdb[4] = ((lba >> 8) & 0xFF) as u8;
    cdb[5] = (lba & 0xFF) as u8;
    cdb[6] = ((this_sectors >> 16) & 0xFF) as u8;
    cdb[7] = ((this_sectors >> 8) & 0xFF) as u8;
    cdb[8] = (this_sectors & 0xFF) as u8;
    cdb[9] = mode.cdb_byte9();

    let handle = crate::windows::get_drive_handle()?;
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
            std::ptr::null_mut(),
        )
    };

    if ok == 0 {
        return Err(CdReaderError::Io(std::io::Error::last_os_error()));
    }
    if wrapper.sptd.ScsiStatus != 0 {
        let (sense_key, asc, ascq) = parse_sense(&wrapper.sense, wrapper.sptd.SenseInfoLength);
        return Err(CdReaderError::Scsi(ScsiError {
            op: ScsiOp::ReadCd,
            lba: Some(lba),
            sectors: Some(this_sectors),
            scsi_status: wrapper.sptd.ScsiStatus,
            sense_key,
            asc,
            ascq,
        }));
    }

    Ok(chunk)
}

#[cfg(target_os = "macos")]
fn read_data_chunk(
    lba: u32,
    this_sectors: u32,
    mode: &SectorReadMode,
) -> Result<Vec<u8>, crate::CdReaderError> {
    crate::macos::read_data_chunk(lba, this_sectors, mode)
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn parse_sense(sense: &[u8], sb_len_wr: u8) -> (Option<u8>, Option<u8>, Option<u8>) {
    if sb_len_wr == 0 || sense.len() < 14 {
        return (None, None, None);
    }
    (Some(sense[2] & 0x0F), Some(sense[12]), Some(sense[13]))
}
