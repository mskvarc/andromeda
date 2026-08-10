//! Notification outbox delivery configuration.

use confique::Config;
use serde::Serialize;
use thiserror::Error;

/// Invalid durable notification outbox configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum OutboxConfigError {
    /// Retry attempts were configured as zero or less.
    #[error("outbox.max_attempts must be at least 1")]
    NonPositiveMaxAttempts,
    /// Retry backoff was configured as zero.
    #[error("outbox.base_backoff_secs must be at least 1")]
    ZeroBaseBackoff,
    /// Polling cadence was configured as zero.
    #[error("outbox.poll_interval_secs must be at least 1")]
    ZeroPollInterval,
    /// Delivery batch size was configured as zero or less.
    #[error("outbox.batch_size must be at least 1")]
    NonPositiveBatchSize,
    /// Delivered-entry retention was configured as zero.
    #[error("outbox.cleanup_after_secs must be at least 1")]
    ZeroCleanupRetention,
    /// Cleanup cadence was configured as zero.
    #[error("outbox.cleanup_interval_secs must be at least 1")]
    ZeroCleanupInterval,
}

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
    /// Cadence for physical cleanup of acknowledged entries.
    #[config(env = "ANDROMEDA_OUTBOX_CLEANUP_INTERVAL_SECS", default = 300)]
    pub cleanup_interval_secs: u64,
}

impl OutboxConfig {
    /// Validate durable notification delivery and cleanup limits.
    pub const fn validate(&self) -> Result<(), OutboxConfigError> {
        if self.max_attempts <= 0 {
            return Err(OutboxConfigError::NonPositiveMaxAttempts);
        }
        if self.batch_size <= 0 {
            return Err(OutboxConfigError::NonPositiveBatchSize);
        }
        if self.base_backoff_secs == 0 {
            return Err(OutboxConfigError::ZeroBaseBackoff);
        }
        if self.poll_interval_secs == 0 {
            return Err(OutboxConfigError::ZeroPollInterval);
        }
        if self.cleanup_after_secs == 0 {
            return Err(OutboxConfigError::ZeroCleanupRetention);
        }
        if self.cleanup_interval_secs == 0 {
            return Err(OutboxConfigError::ZeroCleanupInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{OutboxConfig, OutboxConfigError};

    #[test]
    fn non_positive_batch_size_is_rejected() {
        let config = OutboxConfig {
            enabled: true,
            max_attempts: 5,
            base_backoff_secs: 5,
            poll_interval_secs: 1,
            batch_size: 0,
            cleanup_after_secs: 86_400,
            cleanup_interval_secs: 300,
        };
        assert_eq!(config.validate(), Err(OutboxConfigError::NonPositiveBatchSize));
    }
}
