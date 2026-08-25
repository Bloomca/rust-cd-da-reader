use std::time::Duration;

/// Retry policy for failed drive reads.
///
/// Track and sector-range reads are split into chunks, and this policy is
/// applied independently to each chunk. If a chunk read fails, the next
/// attempt starts at the same LBA. Retry delays use capped exponential backoff.
/// When adaptive chunk reduction is enabled, retries request fewer sectors from
/// that LBA, down to the configured minimum. Chunks that were already read
/// successfully are not repeated.
///
/// The default values are suitable for most drives. Unless you have specific
/// requirements, using [`RetryConfig::default`] is recommended.
///
/// The default policy uses:
///
/// - 4 attempts, including the initial read;
/// - a 20 ms initial backoff;
/// - a 300 ms maximum backoff;
/// - adaptive chunk reduction;
/// - a minimum chunk size of 1 sector.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub(crate) max_attempts: u8,
    pub(crate) initial_backoff: Duration,
    pub(crate) max_backoff: Duration,
    pub(crate) reduce_chunk_on_retry: bool,
    pub(crate) min_sectors_per_read: u32,
}

impl RetryConfig {
    /// Set the maximum attempts per chunk, including the initial read.
    ///
    /// A value of zero is normalized to one attempt.
    pub fn with_max_attempts(mut self, attempts: u8) -> Self {
        self.max_attempts = attempts.max(1);
        self
    }

    /// Set the delay before the second attempt.
    ///
    /// The first attempt is always immediate.
    pub fn with_initial_backoff(mut self, backoff: Duration) -> Self {
        self.initial_backoff = backoff;
        self
    }

    /// Set the upper bound for exponential backoff delays.
    ///
    /// A duration of zero disables retry delays.
    pub fn with_max_backoff(mut self, backoff: Duration) -> Self {
        self.max_backoff = backoff;
        self
    }

    /// Enable or disable requesting fewer sectors after a failed chunk read.
    pub fn with_chunk_reduction(mut self, enabled: bool) -> Self {
        self.reduce_chunk_on_retry = enabled;
        self
    }

    /// Set the minimum sectors per command when adaptive reduction is enabled.
    ///
    /// A value of zero is normalized to one sector.
    pub fn with_min_sectors_per_read(mut self, sectors: u32) -> Self {
        self.min_sectors_per_read = sectors.max(1);
        self
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_backoff: Duration::from_millis(20),
            max_backoff: Duration::from_millis(300),
            reduce_chunk_on_retry: true,
            min_sectors_per_read: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_documented_values() {
        let config = RetryConfig::default();

        assert_eq!(config.max_attempts, 4);
        assert_eq!(config.initial_backoff, Duration::from_millis(20));
        assert_eq!(config.max_backoff, Duration::from_millis(300));
        assert!(config.reduce_chunk_on_retry);
        assert_eq!(config.min_sectors_per_read, 1);
    }

    #[test]
    fn builders_override_and_normalize_values() {
        let config = RetryConfig::default()
            .with_max_attempts(0)
            .with_initial_backoff(Duration::from_millis(50))
            .with_max_backoff(Duration::from_secs(1))
            .with_chunk_reduction(false)
            .with_min_sectors_per_read(0);

        assert_eq!(config.max_attempts, 1);
        assert_eq!(config.initial_backoff, Duration::from_millis(50));
        assert_eq!(config.max_backoff, Duration::from_secs(1));
        assert!(!config.reduce_chunk_on_retry);
        assert_eq!(config.min_sectors_per_read, 1);
    }
}
