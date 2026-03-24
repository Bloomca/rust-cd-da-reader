//! # CD-DA (audio CD) reading library
//!
//! This library provides cross-platform audio CD reading capabilities
//! (tested on Windows, macOS and Linux). It was written to enable CD ripping,
//! but you can also implement a live audio CD player with its help.
//! The library works by issuing direct SCSI commands and abstracts both
//! access to the CD drive and reading the actual data from it, so you don't
//! deal with the hardware directly.
//!
//! All operations happen in this order:
//!
//! 1. Get a CD drive's handle
//! 2. Read the ToC (table of contents) of the audio CD
//! 3. Read track data using ranges from the ToC
//!
//! ## CD access
//!
//! The easiest way to open a drive is to use [`CdReader::open_default`], which scans
//! all drives and opens the first one that contains an audio CD:
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! let reader = CdReader::open_default()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! If you need to pick a specific drive, use [`CdReader::list_drives`] followed
//! by calling [`CdReader::open`] with the specific drive:
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! // Windows / Linux: enumerate drives and inspect the has_audio_cd field
//! let drives = CdReader::list_drives()?;
//!
//! // Any platform: open a known path directly
//! // Windows:  r"\\.\E:"
//! // macOS:    "disk6"
//! // Linux:    "/dev/sr0"
//! let reader = CdReader::open("disk6")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! > **macOS note:** querying drives requires claiming exclusive access, which
//! > unmounts the disc. Releasing it triggers a remount that hands control to
//! > the default app (usually Apple Music). Use `open_default` or `open` with a
//! > known path instead of `list_drives` on macOS.
//!
//! ## Reading ToC
//!
//! Each audio CD carries a Table of Contents with the block address of every
//! track. You need to read it first before issuing any track read commands:
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The returned [`Toc`] contains a [`Vec<Track>`](Track) where each entry has
//! two equivalent address fields:
//!
//! - **`start_lba`** -- Logical Block Address, which is a sector index.
//!   LBA 0 is the first readable sector after the 2-second lead-in pre-gap.
//!   This is the format used internally for read commands.
//! - **`start_msf`** — Minutes/Seconds/Frames, a time-based address inherited
//!   from the physical disc layout. A "frame" is one sector; the spec defines
//!   75 frames per second. MSF includes a fixed 2-second (150-frame) lead-in
//!   offset, so `(0, 2, 0)` corresponds to LBA 0. You can convert between them easily:
//!   `LBA + 150 = total frames`, then divide by 75 and 60 for M/S/F.
//!
//! ## Reading tracks
//!
//! Pass the [`Toc`] and a track number to [`CdReader::read_track`]. The
//! library calculates the sector boundaries automatically:
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//! let data = reader.read_track(&toc, 1)?; // we assume track #1 exists and is audio
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! This is a blocking call. For a live-playback or progress-reporting use case,
//! use the streaming API instead:
//!
//! ```no_run
//! use cd_da_reader::{CdReader, RetryConfig, TrackStreamConfig};
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//!
//! let cfg = TrackStreamConfig {
//!     sectors_per_chunk: 27, // ~64 KB per chunk
//!     retry: RetryConfig::default(),
//! };
//!
//! let mut stream = reader.open_track_stream(&toc, 1, cfg)?;
//! while let Some(chunk) = stream.next_chunk()? {
//!     // process chunk — raw PCM, 2 352 bytes per sector
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Track format
//!
//! Track data is raw [PCM](https://en.wikipedia.org/wiki/Pulse-code_modulation),
//! the same format used inside WAV files. Audio CDs use 16-bit stereo PCM
//! sampled at 44 100 Hz:
//!
//! ```text
//! 44 100 samples * 2 channels * 2 bytes = 176 400 bytes/second
//! ```
//!
//! Each sector holds exactly 2 352 bytes (176 400 ÷ 75 = 2 352), that's where
//! 75 sectors per second comes from. A typical 3-minute track is
//! ~31 MB; a full 74-minute CD is ~650 MB.
//!
//! Converting raw PCM to a playable WAV file only requires prepending a 44-byte
//! RIFF header — [`CdReader::create_wav`] does exactly that:
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//! let data = reader.read_track(&toc, 1)?;
//! let wav = CdReader::create_wav(data);
//! std::fs::write("track01.wav", wav)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Metadata
//!
//! Audio CDs carry almost no semantic metadata. [CD-TEXT] exists but is
//! unreliable and because of that is not provided by this lbirary. The practical approach is to
//! calculate a Disc ID from the ToC and look it up on a service such as
//! [MusicBrainz]. The [`Toc`] struct exposes everything required for the
//! [MusicBrainz disc ID algorithm].
//!
//! [CD-TEXT]: https://en.wikipedia.org/wiki/CD-Text
//! [MusicBrainz]: https://musicbrainz.org/
//! [MusicBrainz disc ID algorithm]: https://musicbrainz.org/doc/Disc_ID_Calculation
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
