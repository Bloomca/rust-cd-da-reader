use std::cmp::min;
use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::IscsiDisc::{
    IOCTL_SCSI_PASS_THROUGH_DIRECT, SCSI_IOCTL_DATA_IN, SCSI_PASS_THROUGH_DIRECT,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::utils::get_track_bounds;
use crate::windows::SptdWithSense;
use crate::{CdReaderError, ScsiError, ScsiOp, Toc};

pub fn read_track(handle: HANDLE, toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdReaderError> {
    let (start_lba, sectors) = get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
    read_cd_audio_range(handle, start_lba, sectors)
}

// --- READ CD (0xBE): read an arbitrary LBA range as CD-DA (2352 bytes/sector) ---
fn read_cd_audio_range(
    handle: HANDLE,
    start_lba: u32,
    sectors: u32,
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

    while remaining > 0 {
        let this_sectors = min(remaining, MAX_SECTORS_PER_XFER);
        let mut chunk = vec![0u8; (this_sectors as usize) * SECTOR_BYTES];

        let mut wrapper: SptdWithSense = unsafe { mem::zeroed() };
        let sptd = &mut wrapper.sptd;

        sptd.Length = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u16;
        sptd.CdbLength = 12; // READ CD is a 12-byte CDB
        sptd.DataIn = SCSI_IOCTL_DATA_IN as u8; // device -> host
        sptd.TimeOutValue = 30; // seconds
        sptd.DataTransferLength = chunk.len() as u32; // 2352 * N
        sptd.DataBuffer = chunk.as_mut_ptr() as *mut _;

        sptd.SenseInfoLength = wrapper.sense.len() as u8;
        sptd.SenseInfoOffset = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u32; // sense follows struct

        // CDB: READ CD (0xBE), LBA addressing
        let cdb = &mut sptd.Cdb;
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

        out.extend_from_slice(&chunk);

        lba += this_sectors;
        remaining -= this_sectors;
    }

    Ok(out)
}

fn parse_sense(sense: &[u8], sense_len: u8) -> (Option<u8>, Option<u8>, Option<u8>) {
    if sense_len < 14 || sense.len() < 14 {
        return (None, None, None);
    }

    (Some(sense[2] & 0x0F), Some(sense[12]), Some(sense[13]))
}
