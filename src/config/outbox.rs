//! Loading boundary for notification outbox configuration.

use andromeda_notification::outbox_config::OutboxConfig as NotificationOutboxConfig;
use confique::Config;
use serde::Serialize;

/// File and environment representation of notification outbox configuration.
#[derive(Config, Debug, Clone, Serialize)]
pub struct OutboxConfig {
    /// Enable durable notification intents.
    #[config(env = "ANDROMEDA_OUTBOX_ENABLED", default = false)]
    pub enabled: bool,
    /// Maximum retry attempts before terminal classification.
    #[config(env = "ANDROMEDA_OUTBOX_MAX_ATTEMPTS", default = 5)]
    pub max_attempts: i32,
    /// Base exponential retry backoff.
    #[config(env = "ANDROMEDA_OUTBOX_BASE_BACKOFF_SECS", default = 5)]
    pub base_backoff_secs: u64,
    /// Fallback cadence for the delivery drain.
    #[config(env = "ANDROMEDA_OUTBOX_POLL_INTERVAL_SECS", default = 1)]
    pub poll_interval_secs: u64,
    /// Entries claimed by one bounded drain pass.
    #[config(env = "ANDROMEDA_OUTBOX_BATCH_SIZE", default = 50)]
    pub batch_size: i32,
    /// Age after which acknowledged entries may be removed.
    #[config(env = "ANDROMEDA_OUTBOX_CLEANUP_AFTER_SECS", default = 86400)]
    pub cleanup_after_secs: u64,
    /// Cadence for physical cleanup of acknowledged entries.
    #[config(env = "ANDROMEDA_OUTBOX_CLEANUP_INTERVAL_SECS", default = 300)]
    pub cleanup_interval_secs: u64,
}

impl OutboxConfig {
    /// Project loading concerns into the notification-owned policy configuration.
    pub const fn policy(&self) -> NotificationOutboxConfig {
        NotificationOutboxConfig {
            enabled: self.enabled,
            max_attempts: self.max_attempts,
            base_backoff_secs: self.base_backoff_secs,
            poll_interval_secs: self.poll_interval_secs,
            batch_size: self.batch_size,
            cleanup_after_secs: self.cleanup_after_secs,
            cleanup_interval_secs: self.cleanup_interval_secs,
        }
    }
}
