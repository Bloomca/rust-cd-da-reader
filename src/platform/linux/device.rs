use libc::{O_NONBLOCK, O_RDWR};
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;

static mut DRIVE_HANDLE: Option<File> = None;

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

pub(crate) fn open_drive(path: &str) -> io::Result<()> {
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

    let drive_handle = unsafe { File::from_raw_fd(fd) };
    unsafe {
        DRIVE_HANDLE = Some(drive_handle);
    }

    Ok(())
}

#[allow(static_mut_refs)]
pub(crate) fn close_drive() {
    unsafe {
        if let Some(current_drive) = DRIVE_HANDLE.take() {
            drop(current_drive);
        }
    }
}

#[allow(static_mut_refs)]
pub(super) fn drive_fd() -> io::Result<RawFd> {
    unsafe {
        DRIVE_HANDLE
            .as_ref()
            .map(AsRawFd::as_raw_fd)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Drive not opened"))
    }
}
