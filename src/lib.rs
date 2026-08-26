//! # CD-DA (audio CD) reading library
//!
//! This library provides cross-platform audio CD reading capabilities (tested
//! on Windows, macOS and Linux). It was written to enable CD ripping, but it can
//! also be used to build a live audio CD player. The primary API reads physical
//! discs; to read from a file, image, or another custom source, implement
//! [`AudioSectorReader`] and provide a [`Toc`].
//!
//! Physical-disc access uses platform CD-drive APIs on macOS and direct SCSI
//! commands on Windows and Linux. The library abstracts both access to the drive
//! and reading the data, so callers do not interact with the hardware directly.
//! It operates entirely in user space.
//!
//! A typical drive-backed read happens in this order:
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
//! by calling [`CdReader::open`] with the selected drive:
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! let drives = CdReader::list_drives()?;
//! let selected = drives
//!     .iter()
//!     .find(|drive| drive.has_audio_cd) // we check for audio by checking ToC
//!     .ok_or("no drive with an audio CD found")?;
//!
//! let reader = CdReader::open(selected)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! If you already know the platform-specific device path, use
//! [`CdReader::open_path`] instead.
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
//! The returned [`Toc`] contains a [`Vec<Track>`](Track). Each [`Track`] reports
//! its disc track number in [`Track::number`] and whether it contains audio in
//! [`Track::is_audio`]. Track numbers are not zero-based indices into
//! [`Toc::tracks`] and are not guaranteed to begin at 1 (but they usually do).
//!
//! Each track also has two equivalent address fields:
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
//! Pass the [`Toc`] and the track's [`Track::number`] to
//! [`CdReader::read_track`]. The library calculates the sector boundaries
//! automatically. On CD-Extra discs
//! where the last audio track is followed only by data tracks, the trailing
//! audio/data session gap is excluded from the audio read -- this is usually
//! what you want, and you can read custom range by using [`CdReader::read_sector_range`].
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//!
//! // Track numbers come from the disc; do not assume the first audio track is #1.
//! let track = toc
//!     .tracks
//!     .iter()
//!     .find(|track| track.is_audio)
//!     .ok_or("no audio tracks found")?;
//! let data = reader.read_track(&toc, track.number)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [`CdReader::read_track`] is a blocking call that buffers the complete track,
//! so it can take some time and use hundreds of megabytes of memory. The
//! streaming API instead returns sector-aligned chunks as they are read, which
//! keeps memory usage low and supports progress reporting or playback before the
//! complete track is available.
//!
//! Streaming is still synchronous: each [`TrackStream::next_chunk`] call waits
//! for the drive to return the next chunk. This is often suitable for a CLI,
//! where the read loop can run on the main thread and report progress. A GUI
//! should run the loop on a worker thread so drive reads do not block its event
//! loop. Open a stream with [`CdReader::open_track_stream`]:
//!
//! ```no_run
//! use cd_da_reader::CdReader;
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//!
//! // Select by track metadata rather than assuming track #1 contains audio.
//! let track = toc
//!     .tracks
//!     .iter()
//!     .find(|track| track.is_audio)
//!     .ok_or("no audio tracks found")?;
//! let mut stream = reader.open_track_stream(&toc, track.number)?;
//! while let Some(chunk) = stream.next_chunk()? {
//!     // process chunk — raw PCM, 2 352 bytes per sector
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Audio track format
//!
//! Audio track data is raw
//! [PCM](https://en.wikipedia.org/wiki/Pulse-code_modulation), the same
//! uncompressed sample representation used by PCM WAV files. Audio CDs use
//! signed 16-bit little-endian stereo PCM sampled at 44,100 Hz:
//!
//! ```text
//! 44,100 sample frames * 2 channels * 2 bytes = 176,400 bytes/second
//! ```
//!
//! Each audio sector holds exactly 2,352 bytes (176,400 ÷ 75 = 2,352), which
//! gives 75 sectors per second. A typical 3-minute track is about 31.8 MB
//! (30.3 MiB). A 74-minute disc contains about 783 MB (747 MiB) of raw PCM;
//! common 80-minute media contains about 847 MB (808 MiB).
//!
//! Converting raw PCM to a playable WAV file only requires prepending a 44-byte
//! RIFF header — [`create_wav`] does exactly that:
//!
//! ```no_run
//! use cd_da_reader::{CdReader, create_wav};
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//! let track = toc
//!     .tracks
//!     .iter()
//!     .find(|track| track.is_audio)
//!     .ok_or("no audio tracks found")?;
//! let data = reader.read_track(&toc, track.number)?;
//! let wav = create_wav(data);
//! let output = format!("track{:02}.wav", track.number);
//! std::fs::write(output, wav)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Read options
//!
//! [`CdReader::read_track`] and [`CdReader::open_track_stream`] use the
//! [`ReadOptions`] defaults: CD-DA audio sectors, the default retry policy, and
//! no read-speed change. These settings are sufficient for most audio reads.
//!
//! For more control, start with `ReadOptions::default()` and pass the configured
//! options to [`CdReader::read_track_with_options`] or [`CdReader::open_track_stream_with_options`].
//! The configurable options are:
//!
//! - **Sector format:** [`SectorReadFormat`] controls the type and layout of
//!   sectors returned by the drive. [`SectorReadFormat::Audio`] is the default.
//!   For a data track, [`CdReader::detect_track_format`] can select an
//!   appropriate default format to pass to [`ReadOptions::with_format`].
//! - **Retry policy:** [`RetryConfig`] controls the number of attempts, retry
//!   delays, and adaptive reduction of the number of sectors requested after a
//!   failed read. Its defaults are suitable for most drives.
//! - **Read speed:** [`ReadSpeed`] requests an automatic or custom drive speed.
//!   The default, [`ReadSpeed::Unchanged`], issues no speed-change request.
//!   Requested speeds are not guaranteed, and this crate does not restore the
//!   previous drive setting afterward. Speed behavior depends on the OS and
//!   drive firmware.
//!
//! ```no_run
//! use cd_da_reader::{CdReader, ReadOptions, ReadSpeed, RetryConfig, SectorReadFormat};
//!
//! let reader = CdReader::open_default()?;
//! let toc = reader.read_toc()?;
//! let track = toc
//!     .tracks
//!     .iter()
//!     .find(|track| track.is_audio)
//!     .ok_or("no audio tracks found")?;
//! let options = ReadOptions::default()
//!     .with_format(SectorReadFormat::Audio)
//!     .with_retry(RetryConfig::default().with_max_attempts(6))
//!     .with_read_speed(ReadSpeed::CustomMultiplier(4));
//! let data = reader.read_track_with_options(&toc, track.number, &options)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Metadata
//!
//! Audio CDs carry almost no semantic metadata. [CD-TEXT] exists but is
//! unreliable and because of that is not provided by this library. The practical approach is to
//! calculate a Disc ID from the ToC and look it up on a service such as
//! [MusicBrainz]. The [`Toc`] struct exposes everything required for the
//! [MusicBrainz disc ID algorithm].
//!
//! [CD-TEXT]: https://en.wikipedia.org/wiki/CD-Text
//! [MusicBrainz]: https://musicbrainz.org/
//! [MusicBrainz disc ID algorithm]: https://musicbrainz.org/doc/Disc_ID_Calculation
mod platform;

mod data_reader;
mod discovery;
mod errors;
mod read_loop;
mod retry;
mod stream;
mod utils;

mod backend;
pub use backend::{
    AudioSectorReader, AudioTrackStream, TrackBounds, open_track_stream, open_track_stream_at,
    open_track_stream_with_bounds, read_track, read_track_with_bounds,
};
pub use data_reader::{ReadOptions, ReadSpeed, SectorReadFormat};
pub use discovery::DriveInfo;
pub use errors::{CdReaderError, ScsiError, ScsiOp};
pub use retry::RetryConfig;
pub use stream::TrackStream;

mod parse_toc;
pub use parse_toc::lba_to_msf;

/// Representation of the track from ToC, purely in terms of data location on the CD.
#[derive(Debug)]
pub struct Track {
    /// Track number from the Table of Contents (read from the CD itself).
    /// It usually starts with 1, but you should read this value directly when
    /// reading raw track data. There might be gaps, and also in the future
    /// there might be hidden track support, which will be located at number 0.
    pub number: u8,
    /// starting offset
    pub start_lba: u32,
    /// Track start address in `(minutes, seconds, frames)` (MSF) form.
    ///
    /// MSF uses 75 frames per second and includes the standard 150-frame
    /// lead-in offset, so LBA 0 corresponds to `(0, 2, 0)`. See [`lba_to_msf`].
    pub start_msf: (u8, u8, u8),
    /// Whether the TOC identifies this as an audio track.
    /// A value of `false` indicates a data track.
    pub is_audio: bool,
}

/// Table of Contents, read directly from the Audio CD. The most important part
/// is the `tracks` vector, which allows you to read raw track data.
///
/// If you read from file/image directly, you need to construct it manually.
#[derive(Debug)]
pub struct Toc {
    /// First track number reported in the TOC header.
    ///
    /// This is a disc track number, not a zero-based index into [`Toc::tracks`].
    /// It does not have to start with 1 and can be up to 99.
    pub first_track: u8,
    /// Helper value with the last track number. You should not use it directly to
    /// iterate over all available tracks, as there might be gaps.
    pub last_track: u8,
    /// List of tracks with LBA and MSF offsets
    pub tracks: Vec<Track>,
    /// LBA at which the lead-out area begins, as reported by the disc TOC.
    ///
    /// Track-bound calculations use this as the upper bound only for the last
    /// entry in [`Toc::tracks`]. If another track follows, its start and any
    /// applicable CD-Extra session-gap handling determine the preceding track's
    /// bound instead. The lead-out LBA is also required to calculate a
    /// MusicBrainz Disc ID.
    pub leadout_lba: u32,
}

/// Prepends a standard 44-byte RIFF/WAVE header to raw CD-DA PCM.
///
/// `data` must already contain headerless, signed 16-bit little-endian,
/// interleaved stereo PCM sampled at 44,100 Hz. This function does not validate
/// or convert the audio data; it only adds a header describing that format.
///
/// PCM returned by [`CdReader::read_track`] or the source-independent
/// [`read_track`] function already has the required format. The returned vector
/// contains a complete WAV file and can be written directly to a `.wav` file.
pub fn create_wav(data: Vec<u8>) -> Vec<u8> {
    let mut header = utils::create_wav_header(data.len() as u32);
    header.extend_from_slice(&data);
    header
}

/// Helper struct to interact with the audio CD. Internally it holds a platform-specific
/// handle to the open CD drive to read from it and it is correctly closed when CDReader
/// is dropped.
pub struct CdReader {
    drive: platform::Drive,
}

impl CdReader {
    /// Opens a drive returned by [`CdReader::list_drives`].
    ///
    /// The reader owns the opened drive until it is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] if the discovered drive path cannot be
    /// opened with the access required for raw drive commands.
    pub fn open(drive: &DriveInfo) -> Result<Self, CdReaderError> {
        Self::open_path(&drive.path)
    }

    /// Opens a CD drive at a platform-specific device path.
    ///
    /// Example paths are `/dev/sr0` on Linux, `disk6` on macOS, and
    /// `\\.\E:` on Windows. The reader owns the opened drive until it is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] if `path` is invalid or the operating
    /// system cannot open it with the required access.
    pub fn open_path(path: &str) -> Result<Self, CdReaderError> {
        Ok(Self {
            drive: platform::Drive::open(path)?,
        })
    }

    /// Builds a reader from an already-open handle to the drive's device node,
    /// instead of opening the path ourselves.
    ///
    /// This exists for privileged access. Reading a raw optical device can
    /// require more rights than the calling process has, and there is no way to
    /// gain them after the fact — the descriptor has to come from somewhere
    /// else. A caller that hits `EPERM` / `EACCES` from [`CdReader::open_path`]
    /// can obtain one through a privilege-escalation helper (macOS
    /// `/usr/libexec/authopen`, a setuid helper, a launchd service) and hand it
    /// over here.
    ///
    /// The handle must refer to the drive's device node — `/dev/rdiskN` on
    /// macOS, `/dev/srN` on Linux — and the reader takes ownership of it,
    /// closing it on drop.
    ///
    /// On Linux the handle must have been opened `O_RDWR`: the SG_IO ioctls
    /// this crate issues are rejected on a read-only descriptor. On macOS
    /// `O_RDONLY` is correct and preferred, since a write-capable open of an
    /// optical device demands exclusivity.
    #[cfg(unix)]
    pub fn from_file(file: std::fs::File) -> Self {
        Self {
            drive: platform::Drive::from_file(file),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_reader() -> Self {
        Self {
            drive: platform::Drive::test_drive(),
        }
    }

    /// Read Table of Contents for the opened drive. You'll likely only need to access
    /// `tracks` from the returned value in order to iterate and read each track's raw data.
    /// Please note that each track in the vector has `number` property, which you should use
    /// when calling `read_track`, as it doesn't start with 0. It is important to do so,
    /// because in the future it might include 0 for the hidden track.
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] or [`CdReaderError::Scsi`] if the drive
    /// command fails, and [`CdReaderError::Parse`] if the returned TOC is
    /// malformed.
    pub fn read_toc(&self) -> Result<Toc, CdReaderError> {
        self.drive.read_toc()
    }

    /// Read an audio track using the default options.
    ///
    /// It returns raw PCM data, but if you want to save it directly and make it playable,
    /// wrap the result with [`create_wav`].
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`CdReader::read_track_with_options`].
    pub fn read_track(&self, toc: &Toc, track_no: u8) -> Result<Vec<u8>, CdReaderError> {
        self.read_track_with_options(toc, track_no, &ReadOptions::default())
    }

    /// Read a complete track using explicit read options.
    ///
    /// # Errors
    ///
    /// - Returns [`CdReaderError::TrackFormatMismatch`] if the selected sector
    ///   format is incompatible with the track.
    /// - Returns [`CdReaderError::Io`] if the track is absent, its bounds are
    ///   invalid, or an operating-system drive operation fails.
    /// - Returns [`CdReaderError::Scsi`] if the drive rejects a read command.
    pub fn read_track_with_options(
        &self,
        toc: &Toc,
        track_no: u8,
        options: &ReadOptions,
    ) -> Result<Vec<u8>, CdReaderError> {
        if let Some(track) = toc.tracks.iter().find(|track| track.number == track_no) {
            data_reader::validate_track_format(track, options.format())?;
        }

        let (start_lba, sectors) =
            utils::get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
        self.read_sector_range(start_lba, sectors, options)
    }

    /// Read an arbitrary range of sectors using explicit read options.
    ///
    /// # Low-level API
    ///
    /// Callers are responsible for providing valid sector boundaries and selecting
    /// a format compatible with the sectors on the disc. Prefer [`CdReader::read_track`]
    /// or [`CdReader::read_track_with_options`] when reading a complete TOC track.
    ///
    /// # Errors
    ///
    /// - Returns [`CdReaderError::Io`] if the range is invalid, the read-speed
    ///   request fails, or an operating-system read fails.
    /// - Returns [`CdReaderError::Scsi`] if the drive rejects a read command.
    pub fn read_sector_range(
        &self,
        start_lba: u32,
        sectors: u32,
        options: &ReadOptions,
    ) -> Result<Vec<u8>, CdReaderError> {
        let format = options.format();
        self.drive.request_read_speed(options.read_speed())?;
        read_loop::read_sectors_chunked(
            start_lba,
            sectors,
            format,
            options.retry(),
            |lba, chunk_sectors| self.drive.read_cd_chunk(lba, chunk_sectors, format),
        )
    }
}
