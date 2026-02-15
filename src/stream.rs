use std::cmp::min;

use crate::{CdReader, CdReaderError, RetryConfig, Toc, utils};

#[derive(Debug, Clone)]
pub struct TrackStreamConfig {
    pub sectors_per_chunk: u32,
    pub retry: RetryConfig,
}

impl Default for TrackStreamConfig {
    fn default() -> Self {
        Self {
            sectors_per_chunk: 27,
            retry: RetryConfig::default(),
        }
    }
}

pub struct TrackStream<'a> {
    reader: &'a CdReader,
    start_lba: u32,
    next_lba: u32,
    remaining_sectors: u32,
    total_sectors: u32,
    cfg: TrackStreamConfig,
}

impl<'a> TrackStream<'a> {
    const SECTORS_PER_SECOND: f32 = 75.0;

    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CdReaderError> {
        self.next_chunk_with(|lba, sectors, retry| {
            self.reader.read_sectors_with_retry(lba, sectors, retry)
        })
    }

    fn next_chunk_with<F>(&mut self, mut read_fn: F) -> Result<Option<Vec<u8>>, CdReaderError>
    where
        F: FnMut(u32, u32, &RetryConfig) -> Result<Vec<u8>, CdReaderError>,
    {
        if self.remaining_sectors == 0 {
            return Ok(None);
        }

        let sectors = min(self.remaining_sectors, self.cfg.sectors_per_chunk.max(1));
        let chunk = read_fn(self.next_lba, sectors, &self.cfg.retry)?;

        self.next_lba += sectors;
        self.remaining_sectors -= sectors;

        Ok(Some(chunk))
    }

    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }

    pub fn consumed_sectors(&self) -> u32 {
        self.total_sectors - self.remaining_sectors
    }

    pub fn current_sector(&self) -> u32 {
        self.consumed_sectors()
    }

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

    pub fn current_seconds(&self) -> f32 {
        self.current_sector() as f32 / Self::SECTORS_PER_SECOND
    }

    pub fn total_seconds(&self) -> f32 {
        self.total_sectors as f32 / Self::SECTORS_PER_SECOND
    }

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
    pub fn open_track_stream<'a>(
        &'a self,
        toc: &Toc,
        track_no: u8,
        cfg: TrackStreamConfig,
    ) -> Result<TrackStream<'a>, CdReaderError> {
        let (start_lba, sectors) =
            utils::get_track_bounds(toc, track_no).map_err(CdReaderError::Io)?;

        Ok(TrackStream {
            reader: self,
            start_lba,
            next_lba: start_lba,
            remaining_sectors: sectors,
            total_sectors: sectors,
            cfg,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TrackStream, TrackStreamConfig};
    use crate::{CdReader, CdReaderError, RetryConfig};

    fn mk_stream(
        start_lba: u32,
        total_sectors: u32,
        sectors_per_chunk: u32,
    ) -> TrackStream<'static> {
        let reader: &'static CdReader = Box::leak(Box::new(CdReader {}));
        TrackStream {
            reader,
            start_lba,
            next_lba: start_lba,
            remaining_sectors: total_sectors,
            total_sectors,
            cfg: TrackStreamConfig {
                sectors_per_chunk,
                retry: RetryConfig::default(),
            },
        }
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
    fn next_chunk_reads_expected_size_and_advances() {
        let mut stream = mk_stream(10_000, 100, 27);
        let mut called = false;

        let chunk = stream
            .next_chunk_with(|lba, sectors, _| {
                called = true;
                assert_eq!(lba, 10_000);
                assert_eq!(sectors, 27);
                Ok(vec![0u8; (sectors as usize) * 2352])
            })
            .unwrap()
            .unwrap();

        assert!(called);
        assert_eq!(chunk.len(), 27 * 2352);
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
