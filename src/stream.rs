use std::cmp::min;

use crate::{CdReader, CdReaderError, ReadOptions, RetryConfig, SectorReadMode, Toc, utils};

/// Options for streamed track reads.
///
/// The defaults read audio sectors in chunks of 27 using the default retry
/// policy. Use the builder methods to override only the options you need.
#[derive(Debug, Clone)]
pub struct TrackStreamOptions {
    sectors_per_chunk: u32,
    mode: SectorReadMode,
    retry: RetryConfig,
}

impl TrackStreamOptions {
    /// Select the sector format requested from the drive.
    pub fn with_mode(mut self, mode: SectorReadMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the retry policy applied to each chunk read.
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    /// Set the target chunk size in sectors.
    ///
    /// The byte size of a chunk also depends on [`SectorReadMode`]. A value of
    /// zero is normalized to one sector.
    pub fn with_sectors_per_chunk(mut self, sectors: u32) -> Self {
        self.sectors_per_chunk = sectors.max(1);
        self
    }
}

impl Default for TrackStreamOptions {
    fn default() -> Self {
        Self {
            sectors_per_chunk: 27,
            mode: SectorReadMode::Audio,
            retry: RetryConfig::default(),
        }
    }
}

/// Track-scoped streaming reader for audio or data sectors.
///
/// You can pull sector-aligned chunks incrementally and seek to track-relative
/// sector or time positions. Create a stream with [`CdReader::open_track_stream`].
pub struct TrackStream<'a> {
    reader: &'a CdReader,
    start_lba: u32,
    next_lba: u32,
    remaining_sectors: u32,
    total_sectors: u32,
    options: TrackStreamOptions,
}

impl<'a> TrackStream<'a> {
    const SECTORS_PER_SECOND: f32 = 75.0;

    /// Read the next chunk of sector data.
    ///
    /// Returns `Ok(None)` when end-of-track is reached. The bytes per sector
    /// depend on the [`SectorReadMode`] selected in [`TrackStreamOptions`].
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CdReaderError> {
        self.next_chunk_with(|lba, sectors, mode, retry| {
            let options = ReadOptions::default()
                .with_mode(mode)
                .with_retry(retry.clone());
            self.reader.read_sector_range(lba, sectors, &options)
        })
    }

    fn next_chunk_with<F>(&mut self, mut read_fn: F) -> Result<Option<Vec<u8>>, CdReaderError>
    where
        F: FnMut(u32, u32, SectorReadMode, &RetryConfig) -> Result<Vec<u8>, CdReaderError>,
    {
        if self.remaining_sectors == 0 {
            return Ok(None);
        }

        let sectors = min(self.remaining_sectors, self.options.sectors_per_chunk);
        let chunk = read_fn(
            self.next_lba,
            sectors,
            self.options.mode,
            &self.options.retry,
        )?;

        self.next_lba += sectors;
        self.remaining_sectors -= sectors;

        Ok(Some(chunk))
    }

    /// Total number of sectors in this track stream.
    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }

    /// Current stream position as a track-relative sector index.
    /// Keep in mind that if you are playing the sound directly, this
    /// is likely not the track's current position because you probably
    /// keep some of the data in your buffer.
    pub fn current_sector(&self) -> u32 {
        self.total_sectors - self.remaining_sectors
    }

    /// Seek to an absolute track-relative sector position.
    ///
    /// Valid range is `0..=total_sectors()`.
    /// If the sector value is higher than the total, it will throw an error.
    pub fn seek_to_sector(&mut self, sector: u32) -> Result<(), CdReaderError> {
        if sector > self.total_sectors {
            return Err(CdReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek sector is out of track bounds",
            )));
        }

        self.next_lba = self.start_lba + sector;
        self.remaining_sectors = self.total_sectors - sector;
        Ok(())
    }

    /// Current stream position in seconds. Functionally equivalent
    /// to "current_sector", but converted to seconds.
    ///
    /// CD addresses advance at `75 sectors = 1 second`.
    pub fn current_seconds(&self) -> f32 {
        self.current_sector() as f32 / Self::SECTORS_PER_SECOND
    }

    /// Total stream duration in seconds. Functionally equivalent
    /// to "total_sectors", but converted to seconds.
    ///
    /// CD addresses advance at `75 sectors = 1 second`.
    pub fn total_seconds(&self) -> f32 {
        self.total_sectors as f32 / Self::SECTORS_PER_SECOND
    }

    /// Seek to an absolute track-relative time position in seconds.
    ///
    /// Input is converted to sector offset and clamped to track bounds.
    pub fn seek_to_seconds(&mut self, seconds: f32) -> Result<(), CdReaderError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(CdReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek seconds must be a finite non-negative number",
            )));
        }

        let target_sector = (seconds * Self::SECTORS_PER_SECOND).round() as u32;
        self.seek_to_sector(target_sector.min(self.total_sectors))
    }
}

impl CdReader {
    /// Open a streaming reader for an audio track using the default options.
    pub fn open_track_stream<'a>(
        &'a self,
        toc: &Toc,
        track_no: u8,
    ) -> Result<TrackStream<'a>, CdReaderError> {
        self.open_track_stream_with_options(toc, track_no, TrackStreamOptions::default())
    }

    /// Open a streaming reader using explicit sector-format, retry, and chunk options.
    ///
    /// Use [`TrackStream::next_chunk`] to pull sector-aligned chunks in the
    /// format selected in [`TrackStreamOptions`].
    pub fn open_track_stream_with_options<'a>(
        &'a self,
        toc: &Toc,
        track_no: u8,
        options: TrackStreamOptions,
    ) -> Result<TrackStream<'a>, CdReaderError> {
        let (start_lba, sectors) =
            utils::get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;

        Ok(TrackStream {
            reader: self,
            start_lba,
            next_lba: start_lba,
            remaining_sectors: sectors,
            total_sectors: sectors,
            options,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TrackStream, TrackStreamOptions};
    use crate::{CdReader, CdReaderError, RetryConfig, SectorReadMode};

    fn mk_stream(
        start_lba: u32,
        total_sectors: u32,
        sectors_per_chunk: u32,
    ) -> TrackStream<'static> {
        let reader: &'static CdReader = Box::leak(Box::new(CdReader::test_reader()));
        TrackStream {
            reader,
            start_lba,
            next_lba: start_lba,
            remaining_sectors: total_sectors,
            total_sectors,
            options: TrackStreamOptions::default().with_sectors_per_chunk(sectors_per_chunk),
        }
    }

    #[test]
    fn options_builders_override_individual_defaults() {
        let retry = RetryConfig::default().with_max_attempts(9);
        let options = TrackStreamOptions::default()
            .with_mode(SectorReadMode::DataRaw)
            .with_retry(retry)
            .with_sectors_per_chunk(0);

        assert_eq!(options.mode, SectorReadMode::DataRaw);
        assert_eq!(options.retry.max_attempts, 9);
        assert_eq!(options.sectors_per_chunk, 1);
    }

    #[test]
    fn seek_to_sector_updates_position() {
        let mut stream = mk_stream(10_000, 1_000, 27);
        stream.seek_to_sector(250).unwrap();

        assert_eq!(stream.current_sector(), 250);
        assert_eq!(stream.next_lba, 10_250);
        assert_eq!(stream.remaining_sectors, 750);
    }

    #[test]
    fn seek_to_sector_returns_error_out_of_bounds() {
        let mut stream = mk_stream(10_000, 1_000, 27);
        let err = stream.seek_to_sector(1_001).unwrap_err();

        match err {
            CdReaderError::Io(io) => assert_eq!(io.kind(), std::io::ErrorKind::InvalidInput),
            _ => panic!("expected Io(InvalidInput)"),
        }
    }

    #[test]
    fn seek_to_seconds_and_time_helpers_work() {
        let mut stream = mk_stream(10_000, 750, 27); // 10 seconds
        assert_eq!(stream.total_seconds(), 10.0);

        stream.seek_to_seconds(2.0).unwrap();
        assert_eq!(stream.current_sector(), 150);
        assert!((stream.current_seconds() - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn seek_to_seconds_rejects_invalid_input() {
        let mut stream = mk_stream(10_000, 750, 27);
        let err = stream.seek_to_seconds(f32::NAN).unwrap_err();
        match err {
            CdReaderError::Io(io) => assert_eq!(io.kind(), std::io::ErrorKind::InvalidInput),
            _ => panic!("expected Io(InvalidInput)"),
        }
    }

    #[test]
    fn next_chunk_uses_configured_mode_and_advances() {
        let mut stream = mk_stream(10_000, 100, 27);
        stream.options = stream.options.with_mode(SectorReadMode::DataCooked);
        let mut called = false;

        let chunk = stream
            .next_chunk_with(|lba, sectors, mode, _| {
                called = true;
                assert_eq!(lba, 10_000);
                assert_eq!(sectors, 27);
                assert_eq!(mode, SectorReadMode::DataCooked);
                Ok(vec![0u8; (sectors as usize) * mode.sector_size()])
            })
            .unwrap()
            .unwrap();

        assert!(called);
        assert_eq!(chunk.len(), 27 * 2048);
        assert_eq!(stream.current_sector(), 27);
        assert_eq!(stream.remaining_sectors, 73);
    }

    #[test]
    fn next_chunk_returns_none_when_finished() {
        let mut stream = mk_stream(10_000, 0, 27);
        let result = stream
            .next_chunk_with(|_, _, _, _| Ok(vec![1, 2, 3]))
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn next_chunk_error_does_not_advance_position() {
        let mut stream = mk_stream(10_000, 100, 27);
        let err = stream
            .next_chunk_with(|_, _, _, _| {
                Err(CdReaderError::Io(std::io::Error::other(
                    "simulated read failure",
                )))
            })
            .unwrap_err();

        match err {
            CdReaderError::Io(io) => assert_eq!(io.kind(), std::io::ErrorKind::Other),
            _ => panic!("expected Io(Other)"),
        }
        assert_eq!(stream.current_sector(), 0);
        assert_eq!(stream.next_lba, 10_000);
        assert_eq!(stream.remaining_sectors, 100);
    }
}
