//! # CD-DA (or audio CD) reading library
//!
//! This library provides cross-platform audio CD reading capability,
//! it works on Windows, macOS and Linux.
//! It is intended to be a low-level library, and only allows you read
//! TOC and tracks, and you need to provide valid CD drive name.
//! Currently, the functionality is very basic, and there is no way to
//! specify subchannel info, access hidden track or read CD text.
//!
//! The library works by issuing direct SCSI commands.
//!
//! ## Example
//!
//! ```
//! use cd_da_reader::CdReader;
//!
//! fn read_cd() -> Result<(), Box<dyn std::error::Error>> {
//!   let reader = CdReader::open(r"\\.\E:")?;
//!   let toc = reader.read_toc()?;
//!   println!("{:#?}", toc);
//!   let data = reader.read_track(&toc, 11)?;
//!   let wav_track = CdReader::create_wav(data);
//!   std::fs::write("myfile.wav", wav_track)?;
//!   Ok(())
//! }
//! ```
//!
//! This function reads an audio CD on Windows, you can check your drive letter
//! in the File Explorer. On macOS, you can run `diskutil list` and look for the
//! Audio CD in the list (it should be something like "disk4"), and on Linux you
//! can check it using `cat /proc/sys/dev/cdrom/info`, it will be like "/dev/sr0".
//!
//! ## Metadata
//!
//! This library does not provide any direct metadata, and audio CDs typically do
//! not carry it by themselves. To obtain it, you'd need to get it from a place like
//! [MusicBrainz](https://musicbrainz.org/). You should have all necessary information
//! in the TOC struct to calculate the audio CD ID.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

mod discovery;
mod errors;
mod retry;
mod stream;
mod utils;
pub use discovery::DriveInfo;
pub use errors::{CdReaderError, ScsiError, ScsiOp};
pub use retry::RetryConfig;
pub use stream::{TrackStream, TrackStreamConfig};

mod parse_toc;

#[cfg(target_os = "windows")]
mod windows_read_track;

/// Representation of the track from TOC, purely in terms of data location on the CD.
#[derive(Debug)]
pub struct Track {
    /// Track number from the Table of Contents (read from the CD itself).
    /// It usually starts with 1, but you should read this value directly when
    /// reading raw track data. There might be gaps, and also in the future
    /// there might be hidden track support, which will be located at number 0.
    pub number: u8,
    /// starting offset, unnecessary to use directly
    pub start_lba: u32,
    /// starting offset, but in (minute, second, frame) format
    pub start_msf: (u8, u8, u8),
    pub is_audio: bool,
}

/// Table of Contents, read directly from the Audio CD. The most important part
/// is the `tracks` vector, which allows you to read raw track data.
#[derive(Debug)]
pub struct Toc {
    /// Helper value with the first track number
    pub first_track: u8,
    /// Helper value with the last track number. You should not use it directly to
    /// iterate over all available tracks, as there might be gaps.
    pub last_track: u8,
    /// List of tracks with LBA and MSF offsets
    pub tracks: Vec<Track>,
    /// Used to calculate number of sectors for the last track. You'll also need this
    /// in order to calculate MusicBrainz ID.
    pub leadout_lba: u32,
}

/// Helper struct to interact with the audio CD. While it doesn't hold any internal data
/// directly, it implements `Drop` trait, so that the CD drive handle is properly closed.
///
/// Please note that you should not read multiple CDs at the same time, and preferably do
/// not use it in multiple threads. CD drives are a physical thing and they really want to
/// have exclusive access, because of that currently only sequential access is supported.
///
/// This is especially true on macOS, where releasing exclusive lock on the audio CD will
/// cause it to remount, and the default application (very likely Apple Music) will get
/// the exclusive access and it will be challenging to implement a reliable waiting strategy.
pub struct CdReader {}

impl CdReader {
    /// Opens a CD drive at the specified path in order to read data.
    ///
    /// It is crucial to call this function and not to create the Reader
    /// by yourself, as each OS needs its own way of handling the drive access.
    ///
    /// You don't need to close the drive, it will be handled automatically
    /// when the `CdReader` is dropped. On macOS, that will cause the CD drive
    /// to be remounted, and the default application (like Apple Music) will
    /// be called.
    ///
    /// # Arguments
    ///
    /// * `path` - The device path (e.g., "/dev/sr0" on Linux, "disk6" on macOS, and r"\\.\E:" on Windows)
    ///
    /// # Errors
    ///
    /// Returns an error if the drive cannot be opened
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

    /// While this is a low-level library and does not include any codecs to compress the audio,
    /// it includes a helper function to convert raw PCM data into a wav file, which is done by
    /// prepending a 44 RIFF bytes header
    ///
    /// # Arguments
    ///
    /// * `data` - vector of bytes received from `read_track` function
    pub fn create_wav(data: Vec<u8>) -> Vec<u8> {
        let mut header = utils::create_wav_header(data.len() as u32);
        header.extend_from_slice(&data);
        header
    }

    /// Read Table of Contents for the opened drive. You'll likely only need to access
    /// `tracks` from the returned value in order to iterate and read each track's raw data.
    /// Please note that each track in the vector has `number` property, which you should use
    /// when calling `read_track`, as it doesn't start with 0. It is important to do so,
    /// because in the future it might include 0 for the hidden track.
    pub fn read_toc(&self) -> Result<Toc, CdReaderError> {
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

    /// Read raw data for the specified track number from the TOC.
    /// It returns raw PCM data, but if you want to save it directly and make it playable,
    /// wrap the result with `create_wav` function, that will prepend a RIFF header and
    /// make it a proper music file.
    pub fn read_track(&self, toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdReaderError> {
        self.read_track_with_retry(toc, track_no, &RetryConfig::default())
    }

    /// Read raw data for the specified track number from the TOC using explicit retry config.
    pub fn read_track_with_retry(
        &self,
        toc: &Toc,
        track_no: u8,
        cfg: &RetryConfig,
    ) -> Result<Vec<u8>, CdReaderError> {
        #[cfg(target_os = "windows")]
        {
            windows::read_track_with_retry(toc, track_no, cfg)
        }

        #[cfg(target_os = "macos")]
        {
            macos::read_track_with_retry(toc, track_no, cfg)
        }

        #[cfg(target_os = "linux")]
        {
            linux::read_track_with_retry(toc, track_no, cfg)
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            compile_error!("Unsupported platform")
        }
    }

    pub(crate) fn read_sectors_with_retry(
        &self,
        start_lba: u32,
        sectors: u32,
        cfg: &RetryConfig,
    ) -> Result<Vec<u8>, CdReaderError> {
        #[cfg(target_os = "windows")]
        {
            windows::read_sectors_with_retry(start_lba, sectors, cfg)
        }

        #[cfg(target_os = "macos")]
        {
            macos::read_sectors_with_retry(start_lba, sectors, cfg)
        }

        #[cfg(target_os = "linux")]
        {
            linux::read_sectors_with_retry(start_lba, sectors, cfg)
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
