use std::{ffi::CString, ptr, slice};
use std::{io, process::Command};
use std::{thread::sleep, time::Duration};

use crate::parse_toc::parse_toc;
use crate::utils::get_track_bounds;
use crate::{CdReaderError, RetryConfig, ScsiError, ScsiOp, Toc};

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

pub fn list_drive_paths() -> io::Result<Vec<String>> {
    let output = Command::new("diskutil").arg("list").output()?;
    if !output.status.success() {
        return Err(io::Error::other("diskutil list failed"));
    }

    let mut paths = Vec::new();
    let mut current_disk: Option<String> = None;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for raw_line in stdout.lines() {
        let line = raw_line.trim();

        if let Some(rest) = line.strip_prefix("/dev/") {
            let disk = rest.split_whitespace().next().unwrap_or_default();
            current_disk = if disk.starts_with("disk") {
                Some(disk.to_string())
            } else {
                None
            };
            continue;
        }

        if line.contains("CD_partition_scheme")
            && let Some(disk) = current_disk.as_ref()
        {
            paths.push(disk.clone());
        }
    }

    paths.sort();
    paths.dedup();
    Ok(paths)
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

pub fn read_track_with_retry(
    toc: &Toc,
    track_no: u8,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    const SECTOR_BYTES: usize = 2352;
    const MAX_SECTORS_PER_XFER: u32 = 27;

    let (start_lba, sectors) = get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
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

    CdReaderError::Io(std::io::Error::other("macOS SCSI command failed"))
}

fn next_chunk_size(current: u32, min_chunk: u32) -> u32 {
    if current > 8 {
        8.max(min_chunk)
    } else {
        min_chunk
    }
}
