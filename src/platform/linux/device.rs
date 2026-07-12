use libc::{O_NONBLOCK, O_RDWR};
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;

pub(crate) struct Drive {
    // file closes the file descriptor on drop automatically
    file: File,
}

impl Drive {
    pub(crate) fn open(path: &str) -> io::Result<Self> {
        let path = CString::new(path).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "drive path contains an interior NUL byte",
            )
        })?;
        let fd = unsafe { libc::open(path.as_ptr(), O_RDWR | O_NONBLOCK) };
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
    let mut drives = Vec::new();

    if let Ok(entries) = std::fs::read_dir("/sys/class/block") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("sr") {
                drives.push(format!("/dev/{name}"));
            }
        }
    }

    if drives.is_empty() && Path::new("/dev/cdrom").exists() {
        drives.push("/dev/cdrom".to_string());
    }

    drives.sort();
    drives.dedup();
    Ok(drives)
}
