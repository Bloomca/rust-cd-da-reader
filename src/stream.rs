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
    next_lba: u32,
    remaining_sectors: u32,
    total_sectors: u32,
    cfg: TrackStreamConfig,
}

impl<'a> TrackStream<'a> {
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CdReaderError> {
        if self.remaining_sectors == 0 {
            return Ok(None);
        }

        let sectors = min(self.remaining_sectors, self.cfg.sectors_per_chunk.max(1));
        let chunk = self
            .reader
            .read_sectors_with_retry(self.next_lba, sectors, &self.cfg.retry)?;

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
            next_lba: start_lba,
            remaining_sectors: sectors,
            total_sectors: sectors,
            cfg,
        })
    }
}
