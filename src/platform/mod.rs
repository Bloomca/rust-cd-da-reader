#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::{close_drive, list_drive_paths, open_drive, read_cd_chunk, read_toc};
#[cfg(target_os = "windows")]
pub(crate) use windows::{close_drive, list_drive_paths, open_drive, read_cd_chunk, read_toc};
