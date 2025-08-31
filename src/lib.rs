#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

mod parse_toc;

#[cfg(target_os = "windows")]
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

pub struct CdReader {}

impl CdReader {
    pub fn open(path: &str) -> std::io::Result<Self> {
        #[cfg(target_os = "windows")]
        {
            windows::open_drive(path)?;
            Ok(Self {})
        }

        #[cfg(target_os = "macos")]
        {
            macos::open_drive(path)?;
            Ok(Self {})
        }

        #[cfg(target_os = "linux")]
        {
            linux::open_drive(path)?;
            Ok(Self {})
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            compile_error!("Unsupported platform")
        }
    }

    pub fn read_toc(&self) -> Result<Toc, std::io::Error> {
        #[cfg(target_os = "windows")]
        {
            windows::read_toc()
        }

        #[cfg(target_os = "macos")]
        {
            macos::read_toc()
        }

        #[cfg(target_os = "linux")]
        {
            linux::read_toc()
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            compile_error!("Unsupported platform")
        }
    }

    pub fn read_track(&self, toc: &Toc, track_no: u8) -> std::io::Result<Vec<u8>> {
        #[cfg(target_os = "windows")]
        {
            windows::read_track(toc, track_no)
        }

        #[cfg(target_os = "macos")]
        {
            macos::read_track(toc, track_no)
        }

        #[cfg(target_os = "linux")]
        {
            linux::read_track(toc, track_no)
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            compile_error!("Unsupported platform")
        }
    }
}

impl Drop for CdReader {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        {
            windows::close_drive();
        }

        #[cfg(target_os = "macos")]
        {
            macos::close_drive();
        }

        #[cfg(target_os = "linux")]
        {
            linux::close_drive();
        }
    }
}
