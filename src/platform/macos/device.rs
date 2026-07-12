use std::ffi::{CStr, CString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::{ptr, slice};

use super::ffi::{MacDriveInfo, cd_free, list_cd_drives, open_cd_raw_device};

pub(crate) struct Drive {
    // file closes the file descriptor on drop automatically
    file: File,
}

impl Drive {
    pub(crate) fn open(path: &str) -> io::Result<Self> {
        let bsd_name = CString::new(path).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "drive path contains an interior NUL byte",
            )
        })?;
        let fd = unsafe { open_cd_raw_device(bsd_name.as_ptr()) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            file: unsafe { File::from_raw_fd(fd) },
        })
    }

    pub(super) fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    #[cfg(test)]
    pub(crate) fn test_drive() -> Self {
        Self {
            file: File::open("/dev/null").expect("could not open /dev/null for tests"),
        }
    }
}

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
