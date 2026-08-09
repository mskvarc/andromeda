//! Notification outbox worker configuration.

use confique::Config;
use serde::Serialize;

/// Retry, polling, and retention policy for the notification outbox.
#[derive(Config, Debug, Clone, Serialize)]
pub struct OutboxConfig {
    /// Enable writing outbox rows during entity mutations.
    #[config(env = "ANDROMEDA_OUTBOX_ENABLED", default = false)]
    pub enabled: bool,
    /// Maximum retry attempts before dead-lettering.
    #[config(env = "ANDROMEDA_OUTBOX_MAX_ATTEMPTS", default = 5)]
    pub max_attempts: i32,
    /// Base exponential retry backoff.
    #[config(env = "ANDROMEDA_OUTBOX_BASE_BACKOFF_SECS", default = 5)]
    pub base_backoff_secs: u64,
    /// Poll interval for the delivery worker.
    #[config(env = "ANDROMEDA_OUTBOX_POLL_INTERVAL_SECS", default = 1)]
    pub poll_interval_secs: u64,
    /// Entries claimed per poll.
    #[config(env = "ANDROMEDA_OUTBOX_BATCH_SIZE", default = 50)]
    pub batch_size: i32,
    /// Age after which delivered entries are removed.
    #[config(env = "ANDROMEDA_OUTBOX_CLEANUP_AFTER_SECS", default = 86400)]
    pub cleanup_after_secs: u64,
}
