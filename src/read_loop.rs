//! Shared retry + chunking loop for `READ CD` reads.
//!
//! Every platform issues the same logical read: split a sector range into
//! drive-safe chunks, read each chunk with capped exponential backoff and
//! adaptive chunk-size reduction, and concatenate the results. Only the
//! single-command read itself (SG_IO on Linux, SPTI on Windows, IOKit on
//! macOS) is platform-specific, so it is injected as a closure.

use std::thread::sleep;
use std::time::Duration;

use crate::data_reader::SectorReadMode;
use crate::{CdReaderError, RetryConfig};

/// Read `sectors` sectors starting at `start_lba` in the given `mode`.
///
/// `read_chunk(lba, sectors)` performs one platform-specific `READ CD`
/// command and returns the raw bytes for that chunk. The loop owns chunk
/// sizing, retries, and backoff so platform code only implements the
/// single-command read.
pub(crate) fn read_sectors_chunked<F>(
    start_lba: u32,
    sectors: u32,
    mode: &SectorReadMode,
    cfg: &RetryConfig,
    mut read_chunk: F,
) -> Result<Vec<u8>, CdReaderError>
where
    F: FnMut(u32, u32) -> Result<Vec<u8>, CdReaderError>,
{
    let total_bytes = (sectors as usize) * mode.sector_size();
    let max_sectors_per_xfer = mode.max_sectors_per_xfer();
    let mut out = Vec::<u8>::with_capacity(total_bytes);

    let mut remaining = sectors;
    let mut lba = start_lba;
    let attempts_total = cfg.max_attempts.max(1);
    let min_chunk = cfg.min_sectors_per_read.max(1);

    while remaining > 0 {
        let mut chunk_sectors = remaining.min(max_sectors_per_xfer);
        let mut backoff_ms = cfg.initial_backoff_ms;
        let mut last_err: Option<CdReaderError> = None;

        for attempt in 1..=attempts_total {
            match read_chunk(lba, chunk_sectors) {
                Ok(chunk) => {
                    out.extend_from_slice(&chunk);
                    lba += chunk_sectors;
                    remaining -= chunk_sectors;
                    last_err = None;
                    break;
                }
                Err(err) => {
                    last_err = Some(err);
                    if attempt == attempts_total {
                        break;
                    }
                    if cfg.reduce_chunk_on_retry && chunk_sectors > min_chunk {
                        chunk_sectors = next_chunk_size(chunk_sectors, min_chunk);
                    }
                    if backoff_ms > 0 {
                        sleep(Duration::from_millis(backoff_ms));
                    }
                    if cfg.max_backoff_ms > 0 {
                        backoff_ms = (backoff_ms.saturating_mul(2)).min(cfg.max_backoff_ms);
                    }
                }
            }
        }

        if let Some(err) = last_err {
            return Err(err);
        }
    }

    Ok(out)
}

/// Shrink the chunk size after a failed read to improve the odds of success,
/// stepping large reads down toward `min_chunk` (for example `27 -> 8 -> 1`).
fn next_chunk_size(current: u32, min_chunk: u32) -> u32 {
    if current > 8 {
        8.max(min_chunk)
    } else {
        min_chunk
    }
}
