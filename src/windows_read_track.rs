use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::IscsiDisc::{
    IOCTL_SCSI_PASS_THROUGH_DIRECT, SCSI_IOCTL_DATA_IN, SCSI_PASS_THROUGH_DIRECT,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::data_reader::SectorReadMode;
use crate::windows::SptdWithSense;
use crate::{CdReaderError, RetryConfig, ScsiError, ScsiOp};

pub fn read_range_with_retry(
    handle: HANDLE,
    start_lba: u32,
    sectors: u32,
    mode: &SectorReadMode,
    cfg: &RetryConfig,
) -> Result<Vec<u8>, CdReaderError> {
    crate::read_loop::read_sectors_chunked(start_lba, sectors, mode, cfg, |lba, chunk_sectors| {
        read_cd_chunk(handle, lba, chunk_sectors, mode)
    })
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
