use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW,
    OPEN_EXISTING,
};

// https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew#return-value
const DRIVE_CDROM: u32 = 5;

static mut DRIVE_HANDLE: Option<HANDLE> = None;

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

#[allow(static_mut_refs)]
pub(crate) fn open_drive(path: &str) -> io::Result<()> {
    unsafe {
        if DRIVE_HANDLE.is_some() {
            return Err(io::Error::other("Drive is already open"));
        }
    }

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

    unsafe {
        DRIVE_HANDLE = Some(handle);
    }

    Ok(())
}

pub(crate) fn close_drive() {
    unsafe {
        if let Some(handle) = DRIVE_HANDLE {
            CloseHandle(handle);
            DRIVE_HANDLE = None;
        }
    }
}

pub(super) fn drive_handle() -> io::Result<HANDLE> {
    unsafe {
        DRIVE_HANDLE.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Drive not opened"))
    }
}
