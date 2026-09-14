//! Retry, polling, retention and diagnostic limits for notification delivery.

use crate::{
    delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError},
    delivery_concurrency::{DeliveryConcurrency, DeliveryConcurrencyError},
    delivery_error_bound::{DeliveryErrorBound, DeliveryErrorBoundError},
    delivery_lease::{LeaseTerm, LeaseTermError},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid notification delivery configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryConfigError {
    /// The attempt ceiling was outside the accepted range.
    #[error("notifications.delivery.max_attempts is invalid: {0}")]
    AttemptCeiling(#[from] DeliveryAttemptCeilingError),
    /// Retry backoff was configured as zero.
    #[error("notifications.delivery.base_backoff_secs must be at least 1")]
    ZeroBaseBackoff,
    /// Polling cadence was configured as zero.
    #[error("notifications.delivery.poll_interval_secs must be at least 1")]
    ZeroPollInterval,
    /// The outbound notification request timeout was configured as zero.
    #[error("notifications.delivery.request_timeout_secs must be at least 1")]
    ZeroRequestTimeout,
    /// Delivery batch size was configured as zero or less.
    #[error("notifications.delivery.batch_size must be at least 1")]
    NonPositiveBatchSize,
    /// Acknowledged-delivery retention was configured as zero.
    #[error("notifications.delivery.cleanup_after_secs must be at least 1")]
    ZeroCleanupRetention,
    /// Cleanup cadence was configured as zero.
    #[error("notifications.delivery.cleanup_interval_secs must be at least 1")]
    ZeroCleanupInterval,
    /// Dead-lettered (exhausted) delivery retention was configured as zero.
    #[error("notifications.delivery.dead_letter_after_secs must be at least 1")]
    ZeroDeadLetterRetention,
    /// Orphaned-event retention was configured as zero.
    #[error("notifications.delivery.orphan_event_after_secs must be at least 1")]
    ZeroOrphanEventRetention,
    /// The claim lease term was outside the accepted range.
    #[error("notifications.delivery.lease_secs is invalid: {0}")]
    LeaseTerm(#[from] LeaseTermError),
    /// The stored-diagnostic bound was outside the accepted range.
    #[error("notifications.delivery.error_max_chars is invalid: {0}")]
    ErrorBound(#[from] DeliveryErrorBoundError),
    /// The in-flight delivery bound was outside the accepted range.
    #[error("notifications.delivery.concurrency is invalid: {0}")]
    Concurrency(#[from] DeliveryConcurrencyError),
}

/// How notifications are delivered to HTTP and MQTT subscribers.
///
/// Both variants deliver notifications; they differ only in durability. The
/// choice is not a boolean on/off switch, so it is a named mode rather than an
/// `enabled` flag.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum DeliveryMode {
    /// Evaluate subscriptions and deliver notifications in-process, without
    /// persistence, retry, or dead-lettering. A crashed broker loses only the
    /// notifications still in flight.
    #[default]
    Live,
    /// Record every notification in a durable queue and deliver it through the
    /// delivery worker, with retry, leasing, dead-lettering, and retention.
    Durable,
}

/// Retry, polling, retention and diagnostic policy for notification delivery.
#[derive(Clone, Deserialize, Serialize, TomlComment, Debug)]
pub struct DeliveryConfig {
    /// Whether notifications are delivered live (in-process) or durably (queued).
    #[toml_comment(inline)]
    pub mode: DeliveryMode,
    /// How long an outbound notification POST may take before it is abandoned.
    ///
    /// A subscription's own Table 5.2.15-1 `timeout` may only shorten this
    /// system bound, never lengthen it. Applies to both delivery modes.
    pub request_timeout_secs: u64,
    /// Maximum delivery attempts before dead-lettering.
    ///
    /// The worker owns this ceiling: the attempt that reaches it is
    /// acknowledged as exhausted and the delivery leaves the claimable set for
    /// good. The queue schema does not repeat the number, so it may be changed
    /// without a migration; the only bound is that it is at least one.
    pub max_attempts: i32,
    /// Base exponential retry backoff.
    pub base_backoff_secs: u64,
    /// Poll interval for the delivery worker.
    pub poll_interval_secs: u64,
    /// Deliveries claimed per poll.
    pub batch_size: i32,
    /// Claimed deliveries sent concurrently within one pass.
    ///
    /// Bounded by [`DeliveryConcurrency::MAXIMUM`]; a pass never keeps more
    /// than this many transport calls in flight at once.
    pub concurrency: u32,
    /// How long a claimed delivery stays leased to its delivery worker.
    ///
    /// A claim that outlives its delivery attempt is what keeps a second worker
    /// from sending the same notification, so this must exceed the longest
    /// single delivery attempt the transport can take; a crashed worker's
    /// deliveries stay unclaimable for the remainder of the term.
    pub lease_secs: u32,
    /// Characters of a failure diagnostic kept when a failed attempt is recorded.
    ///
    /// The delivery schema constrains the stored column, so this may not exceed
    /// [`DeliveryErrorBound::SCHEMA_MAXIMUM_CHARACTERS`].
    pub error_max_chars: u32,
    /// Age after which acknowledged deliveries are removed.
    pub cleanup_after_secs: u64,
    /// Cadence for physical cleanup of acknowledged deliveries.
    pub cleanup_interval_secs: u64,
    /// Age after which a dead-lettered (exhausted) delivery is removed.
    ///
    /// Kept longer than [`Self::cleanup_after_secs`] by default: an exhausted
    /// delivery is retained specifically as evidence of a notification that
    /// never arrived, so it earns a longer look than a delivery that already
    /// succeeded.
    pub dead_letter_after_secs: u64,
    /// Age after which an event with zero referencing deliveries is removed.
    pub orphan_event_after_secs: u64,
}

impl Default for DeliveryConfig {
    fn default() -> DeliveryConfig {
        DeliveryConfig {
            mode: DeliveryMode::Live,
            request_timeout_secs: 10,
            max_attempts: 5,
            base_backoff_secs: 5,
            poll_interval_secs: 1,
            batch_size: 50,
            concurrency: 20,
            lease_secs: 30,
            error_max_chars: 1024,
            cleanup_after_secs: 86400,
            cleanup_interval_secs: 300,
            dead_letter_after_secs: 604_800,
            orphan_event_after_secs: 86400,
        }
    }
}

impl DeliveryConfig {
    /// Validate notification delivery, retention and diagnostic limits.
    ///
    /// # Errors
    ///
    /// Returns the [`DeliveryConfigError`] naming the first setting that is out
    /// of range: a non-positive attempt ceiling or batch size, a zero retry
    /// backoff, poll interval, cleanup retention, cleanup interval,
    /// dead-letter retention or orphan-event retention, or a claim lease term,
    /// stored-diagnostic bound or in-flight delivery bound its own newtype
    /// rejects.
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
        if self.request_timeout_secs == 0 {
            return Err(DeliveryConfigError::ZeroRequestTimeout);
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

    /// Whether notifications are recorded and delivered through the durable queue.
    #[must_use]
    pub const fn is_durable(&self) -> bool {
        matches!(self.mode, DeliveryMode::Durable)
    }

    /// The outbound notification request timeout.
    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }

    /// The configured in-flight delivery bound.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryConcurrencyError`] when the configured concurrency is
    /// zero or above [`DeliveryConcurrency::MAXIMUM`].
    pub const fn delivery_concurrency(&self) -> Result<DeliveryConcurrency, DeliveryConcurrencyError> {
        DeliveryConcurrency::from_count(self.concurrency)
    }

    /// The configured attempt ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryAttemptCeilingError::NonPositiveAttempts`] when the
    /// configured attempt ceiling is zero or negative.
    pub const fn attempt_ceiling(&self) -> Result<DeliveryAttemptCeiling, DeliveryAttemptCeilingError> {
        DeliveryAttemptCeiling::from_attempts(self.max_attempts)
    }

    /// The configured claim lease term.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseTermError`] when the configured lease term is zero
    /// seconds or above the longest accepted term.
    pub const fn lease_term(&self) -> Result<LeaseTerm, LeaseTermError> {
        LeaseTerm::from_seconds(self.lease_secs)
    }

    /// The configured bound on a stored failure diagnostic.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryErrorBoundError`] when the configured bound is zero
    /// characters or above [`DeliveryErrorBound::SCHEMA_MAXIMUM_CHARACTERS`].
    pub const fn error_bound(&self) -> Result<DeliveryErrorBound, DeliveryErrorBoundError> {
        DeliveryErrorBound::from_characters(self.error_max_chars)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_attempt_ceiling::DeliveryAttemptCeilingError,
        delivery_concurrency::DeliveryConcurrencyError,
        delivery_config::{DeliveryConfig, DeliveryConfigError, DeliveryMode},
        delivery_error_bound::DeliveryErrorBoundError,
        delivery_lease::LeaseTermError,
    };
    use pretty_assertions::assert_eq;

    fn valid_config() -> DeliveryConfig {
        DeliveryConfig {
            mode: DeliveryMode::Durable,
            request_timeout_secs: 10,
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

    #[test]
    fn a_zero_request_timeout_is_rejected() {
        let config = DeliveryConfig {
            request_timeout_secs: 0,
            ..valid_config()
        };
        assert_eq!(config.validate(), Err(DeliveryConfigError::ZeroRequestTimeout));
    }

    #[test]
    fn the_delivery_mode_default_is_live() {
        assert_eq!(DeliveryMode::default(), DeliveryMode::Live);
        assert!(!DeliveryConfig::default().is_durable());
    }
}
