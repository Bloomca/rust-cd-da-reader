mod macos;
mod parse_toc;
mod windows;
mod windows_read_track;

#[derive(Debug)]
pub struct Track {
    pub number: u8,
    pub start_lba: u32,
    pub start_msf: (u8, u8, u8), // (minute, second, frame)
    pub is_audio: bool,
}

#[derive(Debug)]
pub struct Toc {
    pub first_track: u8,
    pub last_track: u8,
    pub tracks: Vec<Track>,
}

pub use macos::{mac_read_toc, mac_start_da_guard, mac_stop_da_guard};
pub use windows::CdDevice;
