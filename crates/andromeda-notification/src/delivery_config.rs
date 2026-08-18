//! Retry, polling, retention and diagnostic limits for notification delivery.

use crate::{
    delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError},
    delivery_concurrency::{DeliveryConcurrency, DeliveryConcurrencyError},
    delivery_error_bound::{DeliveryErrorBound, DeliveryErrorBoundError},
    delivery_lease::{LeaseTerm, LeaseTermError},
};
use confique::Config;
use serde::Serialize;
use thiserror::Error;

/// Invalid notification delivery configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryConfigError {
    /// The attempt ceiling was outside the accepted range.
    #[error("notification_delivery.max_attempts is invalid: {0}")]
    AttemptCeiling(#[from] DeliveryAttemptCeilingError),
    /// Retry backoff was configured as zero.
    #[error("notification_delivery.base_backoff_secs must be at least 1")]
    ZeroBaseBackoff,
    /// Polling cadence was configured as zero.
    #[error("notification_delivery.poll_interval_secs must be at least 1")]
    ZeroPollInterval,
    /// Delivery batch size was configured as zero or less.
    #[error("notification_delivery.batch_size must be at least 1")]
    NonPositiveBatchSize,
    /// Acknowledged-delivery retention was configured as zero.
    #[error("notification_delivery.cleanup_after_secs must be at least 1")]
    ZeroCleanupRetention,
    /// Cleanup cadence was configured as zero.
    #[error("notification_delivery.cleanup_interval_secs must be at least 1")]
    ZeroCleanupInterval,
    /// Dead-lettered (exhausted) delivery retention was configured as zero.
    #[error("notification_delivery.dead_letter_after_secs must be at least 1")]
    ZeroDeadLetterRetention,
    /// Orphaned-event retention was configured as zero.
    #[error("notification_delivery.orphan_event_after_secs must be at least 1")]
    ZeroOrphanEventRetention,
    /// The claim lease term was outside the accepted range.
    #[error("notification_delivery.lease_secs is invalid: {0}")]
    LeaseTerm(#[from] LeaseTermError),
    /// The stored-diagnostic bound was outside the accepted range.
    #[error("notification_delivery.error_max_chars is invalid: {0}")]
    ErrorBound(#[from] DeliveryErrorBoundError),
    /// The in-flight delivery bound was outside the accepted range.
    #[error("notification_delivery.concurrency is invalid: {0}")]
    Concurrency(#[from] DeliveryConcurrencyError),
}

/// Retry, polling, retention and diagnostic policy for notification delivery.
#[derive(Clone, Config, Debug, Serialize)]
pub struct DeliveryConfig {
    /// Enable recording notification events and deliveries during entity mutations.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_ENABLED", default = false)]
    pub enabled: bool,
    /// Maximum delivery attempts before dead-lettering.
    ///
    /// The worker owns this ceiling: the attempt that reaches it is
    /// acknowledged as exhausted and the delivery leaves the claimable set for
    /// good. The queue schema does not repeat the number, so it may be changed
    /// without a migration; the only bound is that it is at least one.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_MAX_ATTEMPTS", default = 5)]
    pub max_attempts: i32,
    /// Base exponential retry backoff.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_BASE_BACKOFF_SECS", default = 5)]
    pub base_backoff_secs: u64,
    /// Poll interval for the delivery worker.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_POLL_INTERVAL_SECS", default = 1)]
    pub poll_interval_secs: u64,
    /// Deliveries claimed per poll.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_BATCH_SIZE", default = 50)]
    pub batch_size: i32,
    /// Claimed deliveries sent concurrently within one pass.
    ///
    /// Bounded by [`DeliveryConcurrency::MAXIMUM`]; a pass never keeps more
    /// than this many transport calls in flight at once.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_CONCURRENCY", default = 20)]
    pub concurrency: u32,
    /// How long a claimed delivery stays leased to its delivery worker.
    ///
    /// A claim that outlives its delivery attempt is what keeps a second worker
    /// from sending the same notification, so this must exceed the longest
    /// single delivery attempt the transport can take; a crashed worker's
    /// deliveries stay unclaimable for the remainder of the term.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_LEASE_SECS", default = 30)]
    pub lease_secs: u32,
    /// Characters of a failure diagnostic kept when a failed attempt is recorded.
    ///
    /// The delivery schema constrains the stored column, so this may not exceed
    /// [`DeliveryErrorBound::SCHEMA_MAXIMUM_CHARACTERS`].
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_ERROR_MAX_CHARS", default = 1024)]
    pub error_max_chars: u32,
    /// Age after which acknowledged deliveries are removed.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_CLEANUP_AFTER_SECS", default = 86400)]
    pub cleanup_after_secs: u64,
    /// Cadence for physical cleanup of acknowledged deliveries.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_CLEANUP_INTERVAL_SECS", default = 300)]
    pub cleanup_interval_secs: u64,
    /// Age after which a dead-lettered (exhausted) delivery is removed.
    ///
    /// Kept longer than [`Self::cleanup_after_secs`] by default: an exhausted
    /// delivery is retained specifically as evidence of a notification that
    /// never arrived, so it earns a longer look than a delivery that already
    /// succeeded.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_DEAD_LETTER_AFTER_SECS", default = 604800)]
    pub dead_letter_after_secs: u64,
    /// Age after which an event with zero referencing deliveries is removed.
    #[config(env = "ANDROMEDA_NOTIFICATION_DELIVERY_ORPHAN_EVENT_AFTER_SECS", default = 86400)]
    pub orphan_event_after_secs: u64,
}

impl DeliveryConfig {
    /// Validate notification delivery, retention and diagnostic limits.
    pub const fn validate(&self) -> Result<(), DeliveryConfigError> {
        match self.attempt_ceiling() {
            Ok(_) => (),
            Err(error) => return Err(DeliveryConfigError::AttemptCeiling(error)),
        }
        if self.batch_size <= 0 {
            return Err(DeliveryConfigError::NonPositiveBatchSize);
        }
        if self.base_backoff_secs == 0 {
            return Err(DeliveryConfigError::ZeroBaseBackoff);
        }
        if self.poll_interval_secs == 0 {
            return Err(DeliveryConfigError::ZeroPollInterval);
        }
        if self.cleanup_after_secs == 0 {
            return Err(DeliveryConfigError::ZeroCleanupRetention);
        }
        if self.cleanup_interval_secs == 0 {
            return Err(DeliveryConfigError::ZeroCleanupInterval);
        }
        if self.dead_letter_after_secs == 0 {
            return Err(DeliveryConfigError::ZeroDeadLetterRetention);
        }
        if self.orphan_event_after_secs == 0 {
            return Err(DeliveryConfigError::ZeroOrphanEventRetention);
        }
        match self.lease_term() {
            Ok(_) => (),
            Err(error) => return Err(DeliveryConfigError::LeaseTerm(error)),
        }
        match self.error_bound() {
            Ok(_) => (),
            Err(error) => return Err(DeliveryConfigError::ErrorBound(error)),
        }
        match self.delivery_concurrency() {
            Ok(_) => Ok(()),
            Err(error) => Err(DeliveryConfigError::Concurrency(error)),
        }
    }

    /// The configured in-flight delivery bound.
    pub const fn delivery_concurrency(&self) -> Result<DeliveryConcurrency, DeliveryConcurrencyError> {
        DeliveryConcurrency::from_count(self.concurrency)
    }

    /// The configured attempt ceiling.
    pub const fn attempt_ceiling(&self) -> Result<DeliveryAttemptCeiling, DeliveryAttemptCeilingError> {
        DeliveryAttemptCeiling::from_attempts(self.max_attempts)
    }

    /// The configured claim lease term.
    pub const fn lease_term(&self) -> Result<LeaseTerm, LeaseTermError> {
        LeaseTerm::from_seconds(self.lease_secs)
    }

    /// The configured bound on a stored failure diagnostic.
    pub const fn error_bound(&self) -> Result<DeliveryErrorBound, DeliveryErrorBoundError> {
        DeliveryErrorBound::from_characters(self.error_max_chars)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_attempt_ceiling::DeliveryAttemptCeilingError,
        delivery_concurrency::DeliveryConcurrencyError,
        delivery_config::{DeliveryConfig, DeliveryConfigError},
        delivery_error_bound::DeliveryErrorBoundError,
        delivery_lease::LeaseTermError,
    };

    fn valid_config() -> DeliveryConfig {
        DeliveryConfig {
            enabled: true,
            max_attempts: 5,
            base_backoff_secs: 5,
            poll_interval_secs: 1,
            batch_size: 50,
            concurrency: 20,
            lease_secs: 30,
            error_max_chars: 1_024,
            cleanup_after_secs: 86_400,
            cleanup_interval_secs: 300,
            dead_letter_after_secs: 604_800,
            orphan_event_after_secs: 86_400,
        }
    }

    #[test]
    fn a_non_positive_attempt_ceiling_is_rejected() {
        let config = DeliveryConfig {
            max_attempts: 0,
            ..valid_config()
        };
        assert_eq!(
            config.validate(),
            Err(DeliveryConfigError::AttemptCeiling(DeliveryAttemptCeilingError::NonPositiveAttempts))
        );
    }

    #[test]
    fn an_attempt_ceiling_above_five_is_accepted() {
        let config = DeliveryConfig {
            max_attempts: 12,
            ..valid_config()
        };
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn non_positive_batch_size_is_rejected() {
        let config = DeliveryConfig {
            batch_size: 0,
            ..valid_config()
        };
        assert_eq!(config.validate(), Err(DeliveryConfigError::NonPositiveBatchSize));
    }

    #[test]
    fn zero_lease_term_is_rejected() {
        let config = DeliveryConfig {
            lease_secs: 0,
            ..valid_config()
        };
        assert_eq!(config.validate(), Err(DeliveryConfigError::LeaseTerm(LeaseTermError::ZeroSeconds)));
    }

    #[test]
    fn an_error_bound_above_the_schema_maximum_is_rejected() {
        let config = DeliveryConfig {
            error_max_chars: 4_096,
            ..valid_config()
        };
        assert_eq!(
            config.validate(),
            Err(DeliveryConfigError::ErrorBound(DeliveryErrorBoundError::AboveSchemaMaximum {
                characters: 4_096,
                maximum: 2_048,
            }))
        );
    }

    #[test]
    fn zero_delivery_concurrency_is_rejected() {
        let config = DeliveryConfig {
            concurrency: 0,
            ..valid_config()
        };
        assert_eq!(config.validate(), Err(DeliveryConfigError::Concurrency(DeliveryConcurrencyError::Zero)));
    }

    #[test]
    fn default_shaped_configuration_is_accepted() {
        assert_eq!(valid_config().validate(), Ok(()));
    }
}
