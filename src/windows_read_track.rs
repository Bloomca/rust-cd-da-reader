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

use crate::windows::SptdWithSense;
use crate::data_reader::SectorReadMode;
use crate::{CdReaderError, RetryConfig, ScsiError, ScsiOp};

pub fn read_range_with_retry(
    handle: HANDLE,
    start_lba: u32,
    sectors: u32,
    mode: &SectorReadMode,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    let sector_size = mode.sector_size();
    let max_sectors_per_xfer = mode.max_sectors_per_xfer();

    let total_bytes = (sectors as usize) * sector_size;
    let mut out = Vec::<u8>::with_capacity(total_bytes);

    let mut remaining = sectors;
    let mut lba = start_lba;
    let attempts_total = cfg.max_attempts.max(1);

    while remaining > 0 {
        let mut chunk_sectors = min(remaining, max_sectors_per_xfer);
        let min_chunk = cfg.min_sectors_per_read.max(1);
        let mut backoff_ms = cfg.initial_backoff_ms;
        let mut last_err: Option<CdReaderError> = None;

        for attempt in 1..=attempts_total {
            match read_cd_chunk(handle, lba, chunk_sectors, mode) {
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

fn read_cd_chunk(
    handle: HANDLE,
    lba: u32,
    this_sectors: u32,
    mode: &SectorReadMode,
) -> Result<Vec<u8>, CdReaderError> {
    let sector_size = mode.sector_size();
    let mut chunk = vec![0u8; (this_sectors as usize) * sector_size];

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
    cdb[1] = mode.cdb_byte1();
    cdb[2] = ((lba >> 24) & 0xFF) as u8;
    cdb[3] = ((lba >> 16) & 0xFF) as u8;
    cdb[4] = ((lba >> 8) & 0xFF) as u8;
    cdb[5] = (lba & 0xFF) as u8;
    cdb[6] = ((this_sectors >> 16) & 0xFF) as u8;
    cdb[7] = ((this_sectors >> 8) & 0xFF) as u8;
    cdb[8] = (this_sectors & 0xFF) as u8;
    cdb[9] = mode.cdb_byte9();

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
