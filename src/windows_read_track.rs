use std::cmp::min;
use std::mem;
use std::ptr;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::IscsiDisc::{
    IOCTL_SCSI_PASS_THROUGH_DIRECT, SCSI_IOCTL_DATA_IN, SCSI_PASS_THROUGH_DIRECT,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::utils::get_track_bounds;
use crate::windows::SptdWithSense;
use crate::{CdReaderError, RetryConfig, ScsiError, ScsiOp, Toc};

pub fn read_track(handle: HANDLE, toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdReaderError> {
    read_track_with_retry(handle, toc, track_no, &RetryConfig::default())
}

pub fn read_track_with_retry(
    handle: HANDLE,
    toc: &Toc,
    track_no: u8,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    let (start_lba, sectors) = get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
    read_cd_audio_range(handle, start_lba, sectors, cfg)
}

// --- READ CD (0xBE): read an arbitrary LBA range as CD-DA (2352 bytes/sector) ---
fn read_cd_audio_range(
    handle: HANDLE,
    start_lba: u32,
    sectors: u32,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    // SCSI-2 defines reading data in 2352 bytes chunks
    const SECTOR_BYTES: usize = 2352;

    // read ~64 KBs per request
    const MAX_SECTORS_PER_XFER: u32 = 27; // 27 * 2352 = 63,504 bytes

    let total_bytes = (sectors as usize) * SECTOR_BYTES;
    // allocate the entire necessary size from the beginning to avoid memory realloc
    let mut out = Vec::<u8>::with_capacity(total_bytes);

    let mut remaining = sectors;
    let mut lba = start_lba;
    let attempts_total = cfg.max_attempts.max(1);

    while remaining > 0 {
        let mut chunk_sectors = min(remaining, MAX_SECTORS_PER_XFER);
        let min_chunk = cfg.min_sectors_per_read.max(1);
        let mut backoff_ms = cfg.initial_backoff_ms;
        let mut last_err: Option<CdReaderError> = None;

        for attempt in 1..=attempts_total {
            match read_cd_audio_chunk(handle, lba, chunk_sectors) {
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

fn read_cd_audio_chunk(
    handle: HANDLE,
    lba: u32,
    this_sectors: u32,
) -> Result<Vec<u8>, CdReaderError> {
    const SECTOR_BYTES: usize = 2352;
    let mut chunk = vec![0u8; (this_sectors as usize) * SECTOR_BYTES];

    let mut wrapper: SptdWithSense = unsafe { mem::zeroed() };
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
    cdb[2] = ((lba >> 24) & 0xFF) as u8;
    cdb[3] = ((lba >> 16) & 0xFF) as u8;
    cdb[4] = ((lba >> 8) & 0xFF) as u8;
    cdb[5] = (lba & 0xFF) as u8;
    cdb[6] = ((this_sectors >> 16) & 0xFF) as u8;
    cdb[7] = ((this_sectors >> 8) & 0xFF) as u8;
    cdb[8] = (this_sectors & 0xFF) as u8;
    cdb[9] = 0x10;

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

fn parse_sense(sense: &[u8], sense_len: u8) -> (Option<u8>, Option<u8>, Option<u8>) {
    if sense_len < 14 || sense.len() < 14 {
        return (None, None, None);
    }

    (Some(sense[2] & 0x0F), Some(sense[12]), Some(sense[13]))
}

fn next_chunk_size(current: u32, min_chunk: u32) -> u32 {
    if current > 8 {
        8.max(min_chunk)
    } else {
        min_chunk
    }
}
