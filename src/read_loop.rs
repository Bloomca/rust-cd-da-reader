//! Shared retry + chunking loop for `READ CD` reads.
//!
//! Every platform issues the same logical read: split a sector range into
//! drive-safe chunks, read each chunk with capped exponential backoff and
//! adaptive chunk-size reduction, and concatenate the results. Only the
//! single-command read itself (SG_IO on Linux, SPTI on Windows, IOKit on
//! macOS) is platform-specific, so it is injected as a closure.

use std::thread::sleep;

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
    mode: SectorReadMode,
    cfg: &RetryConfig,
    mut read_chunk: F,
) -> Result<Vec<u8>, CdReaderError>
where
    F: FnMut(u32, u32) -> Result<Vec<u8>, CdReaderError>,
{
    if sectors > 0 && start_lba.checked_add(sectors - 1).is_none() {
        return Err(invalid_input("sector range exceeds the maximum LBA"));
    }

    let total_bytes = (sectors as usize)
        .checked_mul(mode.sector_size())
        .ok_or_else(|| invalid_input("requested byte count is too large"))?;
    let max_sectors_per_xfer = mode.max_sectors_per_xfer();
    let mut out = Vec::<u8>::new();
    out.try_reserve_exact(total_bytes)
        .map_err(|_| invalid_input("could not allocate the requested output buffer"))?;

    let mut remaining = sectors;
    let mut lba = start_lba;
    let attempts_total = cfg.max_attempts.max(1);
    let min_chunk = cfg.min_sectors_per_read.max(1);

    while remaining > 0 {
        let mut chunk_sectors = remaining.min(max_sectors_per_xfer);
        let mut backoff = cfg.initial_backoff.min(cfg.max_backoff);
        let mut last_err: Option<CdReaderError> = None;

        for attempt in 1..=attempts_total {
            let result = read_chunk(lba, chunk_sectors).and_then(|chunk| {
                let expected_len = (chunk_sectors as usize) * mode.sector_size();
                if chunk.len() != expected_len {
                    return Err(CdReaderError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!(
                            "short sector read at LBA {lba}: expected {expected_len} bytes, got {}",
                            chunk.len()
                        ),
                    )));
                }
                Ok(chunk)
            });

            match result {
                Ok(chunk) => {
                    out.extend_from_slice(&chunk);
                    remaining -= chunk_sectors;
                    if remaining > 0 {
                        lba += chunk_sectors;
                    }
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
                    if !backoff.is_zero() {
                        sleep(backoff);
                    }
                    backoff = backoff.saturating_mul(2).min(cfg.max_backoff);
                }
            }
        }

        if let Some(err) = last_err {
            return Err(err);
        }
    }

    Ok(out)
}

fn invalid_input(message: &'static str) -> CdReaderError {
    CdReaderError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message,
    ))
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::read_sectors_chunked;
    use crate::{CdReaderError, RetryConfig, SectorReadMode};

    fn retry_config(max_attempts: u8, reduce_chunk_on_retry: bool) -> RetryConfig {
        RetryConfig::default()
            .with_max_attempts(max_attempts)
            .with_initial_backoff(Duration::ZERO)
            .with_max_backoff(Duration::ZERO)
            .with_chunk_reduction(reduce_chunk_on_retry)
    }

    #[test]
    fn chunks_a_sector_range_and_concatenates_it() {
        let mut calls = Vec::new();
        let data = read_sectors_chunked(
            100,
            60,
            SectorReadMode::Audio,
            &retry_config(1, false),
            |lba, sectors| {
                calls.push((lba, sectors));
                Ok(vec![0xA5; sectors as usize * 2352])
            },
        )
        .unwrap();

        assert_eq!(calls, [(100, 27), (127, 27), (154, 6)]);
        assert_eq!(data.len(), 60 * 2352);
        assert!(data.iter().all(|byte| *byte == 0xA5));
    }

    #[test]
    fn reduces_the_chunk_after_a_failed_attempt() {
        let mut calls = Vec::new();
        let data = read_sectors_chunked(
            100,
            10,
            SectorReadMode::Audio,
            &retry_config(2, true),
            |lba, sectors| {
                calls.push((lba, sectors));
                if calls.len() == 1 {
                    Err(CdReaderError::Io(std::io::Error::other(
                        "simulated failure",
                    )))
                } else {
                    Ok(vec![0; sectors as usize * 2352])
                }
            },
        )
        .unwrap();

        assert_eq!(calls, [(100, 10), (100, 8), (108, 2)]);
        assert_eq!(data.len(), 10 * 2352);
    }

    #[test]
    fn retries_a_short_chunk_without_advancing() {
        let mut calls = Vec::new();
        let data = read_sectors_chunked(
            200,
            2,
            SectorReadMode::DataCooked,
            &retry_config(2, false),
            |lba, sectors| {
                calls.push((lba, sectors));
                if calls.len() == 1 {
                    Ok(vec![0; sectors as usize * 2048 - 1])
                } else {
                    Ok(vec![0; sectors as usize * 2048])
                }
            },
        )
        .unwrap();

        assert_eq!(calls, [(200, 2), (200, 2)]);
        assert_eq!(data.len(), 2 * 2048);
    }

    #[test]
    fn rejects_an_overflowing_lba_range_before_reading() {
        let mut called = false;
        let err = read_sectors_chunked(
            u32::MAX,
            2,
            SectorReadMode::Audio,
            &retry_config(1, false),
            |_, _| {
                called = true;
                Ok(Vec::new())
            },
        )
        .unwrap_err();

        assert!(!called);
        match err {
            CdReaderError::Io(err) => {
                assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput)
            }
            other => panic!("expected invalid-input I/O error, got {other:?}"),
        }
    }
}
