use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW,
    OPEN_EXISTING,
};

// https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew#return-value
const DRIVE_CDROM: u32 = 5;

pub(crate) struct Drive {
    // https://doc.rust-lang.org/beta/std/os/windows/io/struct.OwnedHandle.html
    // OwnedHandle automatically closes the handle on drop
    handle: OwnedHandle,
}

impl Drive {
    pub(crate) fn open(path: &str) -> io::Result<Self> {
        let path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            handle: unsafe { OwnedHandle::from_raw_handle(handle) },
        })
    }

    pub(super) fn handle(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }

    #[cfg(test)]
    pub(crate) fn test_drive() -> Self {
        Self::open("NUL").expect("could not open NUL device for tests")
    }
}

pub(crate) fn list_drive_paths() -> io::Result<Vec<String>> {
    let mut paths = Vec::new();

    for letter in b'A'..=b'Z' {
        let drive = letter as char;
        let root: Vec<u16> = format!("{drive}:\\")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let drive_type = unsafe { GetDriveTypeW(root.as_ptr()) };
        if drive_type == DRIVE_CDROM {
            paths.push(format!(r"\\.\{drive}:"));
        }
    }

    Ok(paths)
}
