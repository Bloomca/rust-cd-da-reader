use windows_sys::Win32::Foundation::{HANDLE, CloseHandle, INVALID_HANDLE_VALUE, GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::Storage::IscsiDisc::{SCSI_PASS_THROUGH_DIRECT, SCSI_IOCTL_DATA_IN, IOCTL_SCSI_PASS_THROUGH_DIRECT};

use std::mem;
use std::ptr;

mod parse_toc;
mod read_track;

#[derive(Debug)]
pub struct Track {
    pub number: u8,
    pub start_lba: u32,
    pub start_msf: (u8, u8, u8),  // (minute, second, frame)
    pub is_audio: bool,
}

#[derive(Debug)]
pub struct Toc {
    pub first_track: u8,
    pub last_track: u8,
    pub tracks: Vec<Track>,
}

#[repr(C)]
pub struct SptdWithSense {
    sptd: SCSI_PASS_THROUGH_DIRECT,
    sense: [u8; 32],
}

pub struct CdDevice {
    handle: HANDLE,
}

impl CdDevice {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let path_wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

        let handle = unsafe { CreateFileW(
            path_wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        ) };

        if handle == INVALID_HANDLE_VALUE {
            println!("Device NOT opened succesfully");
            return Err(std::io::Error::last_os_error());
        }

        println!("Device opened successfully, handle: {:?}", handle);

        Ok(Self { handle })
    }

    pub fn read_toc(&self) -> std::io::Result<Toc> {
        // Buffer that the device will fill with TOC data.
        // 4 + (99 * 8) = 796 max-ish for format 0x00; 2 KiB is safe.
        let alloc_len: usize = 2048;
        let mut data = vec![0u8; alloc_len];

        let mut wrapper: SptdWithSense = unsafe { mem::zeroed() };
        let sptd = &mut wrapper.sptd;

        sptd.Length = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u16;
        sptd.CdbLength = 10; // READ TOC is a 10-byte CDB
        sptd.DataIn = SCSI_IOCTL_DATA_IN as u8;
        sptd.TimeOutValue = 10; // 10 seconds
        sptd.DataTransferLength = alloc_len as u32;
        sptd.DataBuffer = data.as_mut_ptr() as *mut _;

        // Sense buffer immediately follows the struct
        sptd.SenseInfoLength = wrapper.sense.len() as u8;
        // the offset is equal to the first property
        sptd.SenseInfoOffset = size_of::<SCSI_PASS_THROUGH_DIRECT>() as u32;

        // Build CDB for READ TOC/PMA/ATIP (0x43), Format = 0x00 (TOC), MSF = 1
        // CDB layout (10B):
        // [0]=0x43, [1]=MSF bit in bit1, [2]=Format, [6]=StartingTrack,
        // [7..8]=AllocationLength (MSB..LSB), [9]=Control
        let cdb = &mut sptd.Cdb;
        cdb[0] = 0x43;            // READ TOC/PMA/ATIP
        cdb[1] = 0x00;            // LBA format
        cdb[2] = 0x00;            // Format 0x00: TOC
        cdb[6] = 0x00;            // Starting track 0 = first track/session
        cdb[7] = ((alloc_len >> 8) & 0xFF) as u8;
        cdb[8] = (alloc_len & 0xFF) as u8;
        cdb[9] = 0x00;

        let mut bytes = 0u32;

        let ok = unsafe {
            DeviceIoControl(
                self.handle,
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
            return Err(std::io::Error::last_os_error());
        } else if wrapper.sptd.ScsiStatus != 0 {
            eprintln!("SCSI status: 0x{:02X}", wrapper.sptd.ScsiStatus);
            eprintln!("Sense ({} bytes): {:02X?}", wrapper.sense.len(), &wrapper.sense);

            return Err(std::io::Error::last_os_error());
        }

        parse_toc::parse_toc(data)
    }

    pub fn read_track(&self, toc: Toc, track_no: u8) -> std::io::Result<Vec<u8>> {
        read_track::read_track(self.handle, toc, track_no)
    }
}

impl Drop for CdDevice {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}