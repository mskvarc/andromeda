//! Notification outbox delivery configuration.

use serde::Serialize;

/// Retry, polling, and retention policy for the notification outbox.
#[derive(Debug, Clone, Serialize)]
pub struct OutboxConfig {
    /// Enable writing outbox rows during entity mutations.
    pub enabled: bool,
    /// Maximum retry attempts before dead-lettering.
    pub max_attempts: i32,
    /// Base exponential retry backoff.
    pub base_backoff_secs: u64,
    /// Poll interval for the delivery worker.
    pub poll_interval_secs: u64,
    /// Entries claimed per poll.
    pub batch_size: i32,
    /// Age after which delivered entries are removed.
    pub cleanup_after_secs: u64,
    /// Cadence for physical cleanup of acknowledged entries.
    pub cleanup_interval_secs: u64,
}
