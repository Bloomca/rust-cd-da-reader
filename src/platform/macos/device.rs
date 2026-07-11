use std::ffi::{CStr, CString};
use std::io;
use std::{ptr, slice};

use super::ffi::{MacDriveInfo, cd_free, close_dev_session, list_cd_drives, open_dev_session};

pub(crate) fn list_drive_paths() -> io::Result<Vec<String>> {
    let mut raw_drives: *mut MacDriveInfo = ptr::null_mut();
    let mut count = 0u32;

    let success = unsafe { list_cd_drives(&mut raw_drives, &mut count) };
    if !success {
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

            drives.push(path);
        }

        drives.sort();
        drives.dedup();
        drives
    };

    unsafe { cd_free(raw_drives.cast()) };

    Ok(drives)
}

pub(crate) fn open_drive(path: &str) -> io::Result<()> {
    let bsd_name = CString::new(path).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "drive path contains an interior NUL byte",
        )
    })?;
    let success = unsafe { open_dev_session(bsd_name.as_ptr()) };

    if !success {
        return Err(io::Error::other("could not get device"));
    }

    Ok(())
}

pub(crate) fn close_drive() {
    unsafe { close_dev_session() };
}
