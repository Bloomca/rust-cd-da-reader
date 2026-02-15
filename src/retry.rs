/// Retry policy for idempotent read operations.
///
/// The policy is applied per failed read chunk/command and can combine
/// capped exponential backoff with adaptive chunk-size reduction.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum attempts per operation, including the initial attempt.
    /// By default the value is 4.
    ///
    /// Values below `1` are normalized by callers to at least one attempt.
    pub max_attempts: u8,
    /// Initial backoff delay in milliseconds before the second attempt.
    /// First attempt is always immediate, so if there are no issues during
    /// reading, we don't wait any time.
    pub initial_backoff_ms: u64,
    /// Upper bound for exponential backoff delay in milliseconds.
    ///
    /// Each retry typically doubles the previous delay until this cap is reached.
    pub max_backoff_ms: u64,
    /// Enable adaptive sector-count reduction on retry for `READ CD` operations.
    ///
    /// Current implementation reduces chunk size from large reads toward smaller
    /// reads (for example `27 -> 8 -> 1`) to have a higher change of success.
    pub reduce_chunk_on_retry: bool,
    /// Minimum sectors per `READ CD` command when adaptive reduction is enabled.
    /// Default value is 27 for 64KB per read.
    ///
    /// Use `1` for maximal fault isolation; larger values can improve throughput.
    pub min_sectors_per_read: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_backoff_ms: 20,
            max_backoff_ms: 300,
            reduce_chunk_on_retry: true,
            min_sectors_per_read: 1,
        }
    }
}
