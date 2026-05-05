use std::ffi::{CStr, CString};
use std::io;
use std::{ptr, slice};
use std::{thread::sleep, time::Duration};

use crate::parse_toc::parse_toc;
use crate::utils::get_track_bounds;
use crate::{CdReaderError, DriveInfo, RetryConfig, ScsiError, ScsiOp, Toc};

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
    fn read_cd_audio(
        lba: u32,
        sectors: u32,
        out_buf: *mut *mut u8,
        out_len: *mut u32,
        out_err: *mut MacScsiError,
    ) -> bool;
    fn read_cd_data(
        lba: u32,
        sectors: u32,
        cdb_byte1: u8,
        cdb_byte9: u8,
        sector_size: u32,
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

pub fn read_track_with_retry(
    toc: &Toc,
    track_no: u8,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    let (start_lba, sectors) = get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
    read_sectors_with_retry(start_lba, sectors, cfg)
}

pub fn read_sectors_with_retry(
    start_lba: u32,
    sectors: u32,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    const SECTOR_BYTES: usize = 2352;
    const MAX_SECTORS_PER_XFER: u32 = 27;

    let mut out = Vec::<u8>::with_capacity((sectors as usize) * SECTOR_BYTES);
    let mut remaining = sectors;
    let mut lba = start_lba;
    let attempts_total = cfg.max_attempts.max(1);
    let min_chunk = cfg.min_sectors_per_read.max(1);

    while remaining > 0 {
        let mut chunk_sectors = remaining.min(MAX_SECTORS_PER_XFER);
        let mut backoff_ms = cfg.initial_backoff_ms;
        let mut last_err: Option<CdReaderError> = None;

        for attempt in 1..=attempts_total {
            match read_cd_audio_chunk(lba, chunk_sectors) {
                Ok(chunk) => {
                    out.extend_from_slice(&chunk);
                    lba += chunk_sectors;
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
                        sleep(Duration::from_millis(backoff_ms));
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

fn read_cd_audio_chunk(lba: u32, sectors: u32) -> Result<Vec<u8>, CdReaderError> {
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;
    let mut err: MacScsiError = Default::default();
    let ok = unsafe { read_cd_audio(lba, sectors, &mut buf, &mut len, &mut err) };

    if !ok {
        return Err(map_mac_error(err, ScsiOp::ReadCd, Some(lba), Some(sectors)));
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

    CdReaderError::Io(std::io::Error::other("macOS CD command failed"))
}

pub(crate) fn read_data_chunk(
    lba: u32,
    sectors: u32,
    mode: &crate::data_reader::SectorReadMode,
) -> Result<Vec<u8>, CdReaderError> {
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;
    let mut err: MacScsiError = Default::default();
    let ok = unsafe {
        read_cd_data(
            lba,
            sectors,
            mode.cdb_byte1(),
            mode.cdb_byte9(),
            mode.sector_size() as u32,
            &mut buf,
            &mut len,
            &mut err,
        )
    };

    if !ok {
        return Err(map_mac_error(err, ScsiOp::ReadCd, Some(lba), Some(sectors)));
    }

    let data = unsafe { slice::from_raw_parts(buf, len as usize) };
    let result = data.to_vec();
    unsafe { cd_free(buf as *mut _) };
    Ok(result)
}

fn next_chunk_size(current: u32, min_chunk: u32) -> u32 {
    if current > 8 {
        8.max(min_chunk)
    } else {
        min_chunk
    }
}
