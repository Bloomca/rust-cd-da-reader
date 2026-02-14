#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u8,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub reduce_chunk_on_retry: bool,
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
