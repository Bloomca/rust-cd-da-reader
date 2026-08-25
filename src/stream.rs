use std::cmp::min;

use crate::data_reader::validate_track_format;
use crate::{CdReader, CdReaderError, ReadOptions, ReadSpeed, Toc, utils};

fn apply_stream_read_speed_once(
    options: &ReadOptions,
    request_read_speed: impl FnOnce(ReadSpeed) -> Result<(), CdReaderError>,
) -> Result<ReadOptions, CdReaderError> {
    request_read_speed(options.read_speed())?;

    // The speed request applies to the stream as a whole. Chunk reads go
    // through read_sector_range, so to avoid constant speed setting, we
    // apply it once and clone ReadOptions with unchanged speed
    Ok(options.clone().with_read_speed(ReadSpeed::Unchanged))
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
    sectors_per_chunk: u32,
    read_options: ReadOptions,
}

impl<'a> TrackStream<'a> {
    const DEFAULT_SECTORS_PER_CHUNK: u32 = 27;
    const SECTORS_PER_SECOND: f32 = 75.0;

    /// Set the target chunk size in sectors (default 27).
    ///
    /// The byte size of a chunk also depends on the
    /// [`SectorReadFormat`](crate::SectorReadFormat) selected in [`ReadOptions`].
    /// A value of zero is normalized to one sector.
    pub fn with_sectors_per_chunk(mut self, sectors: u32) -> Self {
        self.sectors_per_chunk = sectors.max(1);
        self
    }

    /// Read the next chunk of sector data.
    ///
    /// Returns `Ok(None)` when end-of-track is reached. The bytes per sector
    /// depend on the [`SectorReadFormat`](crate::SectorReadFormat) selected in
    /// [`ReadOptions`].
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] or [`CdReaderError::Scsi`] if the drive
    /// read fails. The stream position does not advance on error.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CdReaderError> {
        self.next_chunk_with(|lba, sectors, options| {
            self.reader.read_sector_range(lba, sectors, options)
        })
    }

    fn next_chunk_with<F>(&mut self, mut read_fn: F) -> Result<Option<Vec<u8>>, CdReaderError>
    where
        F: FnMut(u32, u32, &ReadOptions) -> Result<Vec<u8>, CdReaderError>,
    {
        if self.remaining_sectors == 0 {
            return Ok(None);
        }

        let sectors = min(self.remaining_sectors, self.sectors_per_chunk);
        let chunk = read_fn(self.next_lba, sectors, &self.read_options)?;

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

    /// Seek to a sector position relative to the start of the track.
    ///
    /// Valid range is `0..=total_sectors()`.
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] containing
    /// [`std::io::ErrorKind::InvalidInput`] if `sector` exceeds the track
    /// length.
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

    /// Seek to a time position relative to the start of the track in seconds.
    ///
    /// Input is converted to sector offset and clamped to track bounds.
    ///
    /// # Errors
    ///
    /// Returns [`CdReaderError::Io`] containing
    /// [`std::io::ErrorKind::InvalidInput`] if `seconds` is negative or not
    /// finite.
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
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`CdReader::open_track_stream_with_options`].
    pub fn open_track_stream<'a>(
        &'a self,
        toc: &Toc,
        track_no: u8,
    ) -> Result<TrackStream<'a>, CdReaderError> {
        self.open_track_stream_with_options(toc, track_no, &ReadOptions::default())
    }

    /// Open a streaming reader using explicit read options.
    ///
    /// Use [`TrackStream::next_chunk`] to pull sector-aligned chunks in the
    /// selected format. The requested read speed is applied once before the
    /// stream is returned. To override the default
    /// chunk size, call [`TrackStream::with_sectors_per_chunk`] on the returned
    /// stream.
    ///
    /// # Errors
    ///
    /// - Returns [`CdReaderError::TrackFormatMismatch`] if the selected format
    ///   is incompatible with the track.
    /// - Returns [`CdReaderError::Io`] if the track is absent, its bounds are
    ///   invalid, or the read-speed request fails.
    pub fn open_track_stream_with_options<'a>(
        &'a self,
        toc: &Toc,
        track_no: u8,
        options: &ReadOptions,
    ) -> Result<TrackStream<'a>, CdReaderError> {
        if let Some(track) = toc.tracks.iter().find(|track| track.number == track_no) {
            validate_track_format(track, options.format())?;
        }

        let (start_lba, sectors) =
            utils::get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;
        let read_options = apply_stream_read_speed_once(options, |read_speed| {
            self.drive.request_read_speed(read_speed)
        })?;

        Ok(TrackStream {
            reader: self,
            start_lba,
            next_lba: start_lba,
            remaining_sectors: sectors,
            total_sectors: sectors,
            sectors_per_chunk: TrackStream::DEFAULT_SECTORS_PER_CHUNK,
            read_options,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TrackStream, apply_stream_read_speed_once};
    use crate::{
        CdReader, CdReaderError, ReadOptions, ReadSpeed, RetryConfig, SectorReadFormat, Toc, Track,
    };

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
            sectors_per_chunk: TrackStream::DEFAULT_SECTORS_PER_CHUNK,
            read_options: ReadOptions::default(),
        }
        .with_sectors_per_chunk(sectors_per_chunk)
    }

    #[test]
    fn sectors_per_chunk_normalizes_zero() {
        let stream = mk_stream(10_000, 100, 0);
        assert_eq!(stream.sectors_per_chunk, 1);
    }

    #[test]
    fn stream_speed_is_requested_once_before_chunk_reads() {
        let options = ReadOptions::default().with_read_speed(ReadSpeed::CustomMultiplier(4));
        let mut speed_requests = 0;
        let chunk_options = apply_stream_read_speed_once(&options, |read_speed| {
            speed_requests += 1;
            assert!(matches!(read_speed, ReadSpeed::CustomMultiplier(4)));
            Ok(())
        })
        .unwrap();

        assert_eq!(speed_requests, 1);
        assert!(matches!(chunk_options.read_speed(), ReadSpeed::Unchanged));

        let mut stream = mk_stream(10_000, 100, 27);
        stream.read_options = chunk_options;
        for _ in 0..2 {
            stream
                .next_chunk_with(|_, _, options| {
                    assert!(matches!(options.read_speed(), ReadSpeed::Unchanged));
                    Ok(Vec::new())
                })
                .unwrap();
        }

        assert_eq!(speed_requests, 1);
    }

    #[test]
    fn open_stream_preserves_chunk_read_options() {
        let reader = CdReader::test_reader();
        let toc = Toc {
            first_track: 1,
            last_track: 1,
            tracks: vec![Track {
                number: 1,
                start_lba: 10_000,
                start_msf: (2, 15, 25),
                is_audio: false,
            }],
            leadout_lba: 10_100,
        };
        let options = ReadOptions::default()
            .with_format(SectorReadFormat::Mode1Cooked)
            .with_retry(RetryConfig::default().with_max_attempts(9));

        let stream = reader
            .open_track_stream_with_options(&toc, 1, &options)
            .unwrap();

        assert_eq!(stream.read_options.format(), SectorReadFormat::Mode1Cooked);
        assert_eq!(stream.read_options.retry().max_attempts, 9);
        assert!(matches!(
            stream.read_options.read_speed(),
            ReadSpeed::Unchanged
        ));
        assert_eq!(
            stream.sectors_per_chunk,
            TrackStream::DEFAULT_SECTORS_PER_CHUNK
        );
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
    fn next_chunk_uses_configured_read_options_and_advances() {
        let mut stream = mk_stream(10_000, 100, 27);
        stream.read_options = ReadOptions::default()
            .with_format(SectorReadFormat::Mode1Cooked)
            .with_retry(RetryConfig::default().with_max_attempts(9));
        let mut called = false;

        let chunk = stream
            .next_chunk_with(|lba, sectors, options| {
                called = true;
                assert_eq!(lba, 10_000);
                assert_eq!(sectors, 27);
                assert_eq!(options.format(), SectorReadFormat::Mode1Cooked);
                assert_eq!(options.retry().max_attempts, 9);
                assert!(matches!(options.read_speed(), ReadSpeed::Unchanged));
                Ok(vec![
                    0u8;
                    (sectors as usize) * options.format().sector_size()
                ])
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
        let result = stream.next_chunk_with(|_, _, _| Ok(vec![1, 2, 3])).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn next_chunk_error_does_not_advance_position() {
        let mut stream = mk_stream(10_000, 100, 27);
        let err = stream
            .next_chunk_with(|_, _, _| {
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
