#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::{Drive, list_drive_paths};
#[cfg(target_os = "macos")]
pub(crate) use macos::{Drive, list_drive_paths};
#[cfg(target_os = "windows")]
pub(crate) use windows::{Drive, list_drive_paths};

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("Unsupported platform");
