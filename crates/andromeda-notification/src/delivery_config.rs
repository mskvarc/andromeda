//! Retry, polling, retention and diagnostic limits for notification delivery.

use crate::{
    delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError},
    delivery_concurrency::{DeliveryConcurrency, DeliveryConcurrencyError},
    delivery_error_bound::{DeliveryErrorBound, DeliveryErrorBoundError},
    delivery_lease::{LeaseTerm, LeaseTermError},
};
use andromeda_configuration_scalars::human_duration::HumanDuration;
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// How much longer than `request_timeout` a delivery lease must run.
///
/// One lease covers one delivery attempt from its claim to its
/// acknowledgement. The POST is bounded by `request_timeout`, but the attempt
/// also re-reads its subscription and the entity it notifies before sending,
/// and writes its acknowledgement after: three store round trips the timeout
/// does not bound. This margin is what they may take together before a second
/// worker could reclaim the delivery and send it again.
pub const DELIVERY_LEASE_MARGIN: Duration = Duration::from_secs(5);

/// Invalid notification delivery configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryConfigError {
    /// The attempt ceiling was outside the accepted range.
    #[error("notifications.delivery.max_attempts is invalid: {0}")]
    AttemptCeiling(#[from] DeliveryAttemptCeilingError),
    /// The retry backoff does not bound a retry schedule.
    #[error("notifications.delivery.base_backoff does not form a retry schedule: {0}")]
    RetrySchedule(#[source] BackoffScheduleError),
    /// Polling cadence was configured as zero.
    #[error("notifications.delivery.poll_interval must be greater than zero")]
    ZeroPollInterval,
    /// The idle polling ceiling was configured below the polling floor.
    #[error("notifications.delivery.poll_interval_max must be at least notifications.delivery.poll_interval")]
    PollIntervalCeilingBelowFloor,
    /// The outbound notification request timeout was configured as zero.
    #[error("notifications.delivery.request_timeout must be greater than zero")]
    ZeroRequestTimeout,
    /// Delivery batch size was configured as zero or less.
    #[error("notifications.delivery.batch_size must be at least 1")]
    NonPositiveBatchSize,
    /// Acknowledged-delivery retention was configured as zero.
    #[error("notifications.delivery.cleanup_after must be greater than zero")]
    ZeroCleanupRetention,
    /// Cleanup cadence was configured as zero.
    #[error("notifications.delivery.cleanup_interval must be greater than zero")]
    ZeroCleanupInterval,
    /// Dead-lettered (exhausted) delivery retention was configured as zero.
    #[error("notifications.delivery.dead_letter_after must be greater than zero")]
    ZeroDeadLetterRetention,
    /// Orphaned-event retention was configured as zero.
    #[error("notifications.delivery.orphan_event_after must be greater than zero")]
    ZeroOrphanEventRetention,
    /// The claim lease term was outside the accepted range.
    #[error("notifications.delivery.lease is invalid: {0}")]
    LeaseTerm(#[from] LeaseTermError),
    /// The claim lease term did not exceed the outbound notification request
    /// timeout by [`DELIVERY_LEASE_MARGIN`], so a delivery still in flight
    /// could be claimed and sent again by a second worker.
    ///
    /// A delivery is claimed only when a slot is free to send it, so one lease
    /// covers one delivery attempt and the batch size does not enter this bound.
    #[error(
        "notifications.delivery.lease must exceed notifications.delivery.request_timeout by at least {margin_seconds}s, the store round trips around one delivery attempt"
    )]
    LeaseWithoutMarginOverRequestTimeout {
        /// The margin the lease must leave over the request timeout, in seconds.
        margin_seconds: u64,
    },
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
    /// notifications still in flight. Deliveries a durable broker already
    /// queued in the tenant database are still drained, with their retry.
    #[default]
    Live,
    /// Record every notification in a durable queue and deliver it through the
    /// delivery worker, with retry, leasing, dead-lettering, and retention.
    Durable,
}

/// Retry, polling, retention and diagnostic policy for notification delivery.
///
/// `mode` decides only how a commit's notifications reach their subscribers.
/// The queue-draining settings -- `lease`, `max_attempts`, `base_backoff`,
/// `poll_interval`, `poll_interval_max`, `batch_size`, `concurrency`,
/// `error_max_chars`, `cleanup_after`, `cleanup_interval`, `dead_letter_after`
/// and `orphan_event_after` -- govern the delivery drain, which runs under
/// either mode: the queue lives in the tenant database, and a durable broker
/// sharing it, or this broker before it was switched to live delivery, may
/// have queued deliveries that are still due. `request_timeout` bounds every
/// outbound notification, queued or live.
#[derive(Clone, Deserialize, Serialize, TomlComment, Debug)]
#[serde(deny_unknown_fields)]
pub struct DeliveryConfig {
    /// Whether notifications are delivered live (in-process) or durably (queued).
    #[toml_comment(inline)]
    pub mode: DeliveryMode,
    /// How long an outbound notification POST may take before it is abandoned,
    /// in human-readable form (`"10s"`).
    ///
    /// A subscription's own Table 5.2.15-1 `timeout` may only shorten this
    /// system bound, never lengthen it.
    #[toml_comment(inline)]
    pub request_timeout: HumanDuration,
    /// Maximum delivery attempts before dead-lettering.
    ///
    /// The worker owns this ceiling: the attempt that reaches it is
    /// acknowledged as exhausted and the delivery leaves the claimable set for
    /// good. The queue schema does not repeat the number, so it may be changed
    /// without a migration; the only bound is that it is at least one.
    pub max_attempts: i32,
    /// Base exponential retry backoff, in human-readable form (`"5s"`).
    ///
    /// The wait doubles with every failed attempt until the attempt ceiling.
    #[toml_comment(inline)]
    pub base_backoff: HumanDuration,
    /// Poll interval for the delivery worker: the floor of its idle backoff, in
    /// human-readable form (`"1s"`).
    ///
    /// After a commit's edge wake the worker delivers promptly; while idle it
    /// backs off from here toward `poll_interval_max`.
    #[toml_comment(inline)]
    pub poll_interval: HumanDuration,
    /// Ceiling the delivery worker's idle backoff reaches while no commit wakes
    /// it, in human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub poll_interval_max: HumanDuration,
    /// Deliveries one pass sends at most.
    ///
    /// A pass claims them one slot at a time: one per slot to open, then one
    /// more each time a delivery is acknowledged and frees its slot.
    pub batch_size: i32,
    /// Claimed deliveries sent concurrently within one pass: the slots a pass
    /// keeps busy.
    ///
    /// Bounded by [`DeliveryConcurrency::MAXIMUM`]; a pass never keeps more
    /// than this many transport calls in flight at once.
    pub concurrency: u32,
    /// How long a claimed delivery stays leased to its delivery worker, in
    /// human-readable form (`"30s"`).
    ///
    /// A claim that outlives its delivery attempt is what keeps a second worker
    /// from sending the same notification. A delivery is claimed only when a
    /// slot is free to send it, so one lease has to cover a single delivery
    /// attempt, never the deliveries sent before it: this is refused unless it
    /// exceeds `request_timeout` by [`DELIVERY_LEASE_MARGIN`], whatever
    /// `batch_size` is. A crashed worker's deliveries stay unclaimable for the
    /// remainder of the term.
    #[toml_comment(inline)]
    pub lease: HumanDuration,
    /// Characters of a failure diagnostic kept when a failed attempt is recorded.
    ///
    /// The delivery schema constrains the stored column, so this may not exceed
    /// [`FAILURE_SUMMARY_MAX_CHARACTERS`](andromeda_context_commit::failure_summary::FAILURE_SUMMARY_MAX_CHARACTERS).
    pub error_max_chars: u32,
    /// Age after which acknowledged deliveries are removed, in human-readable
    /// form (`"1day"`).
    #[toml_comment(inline)]
    pub cleanup_after: HumanDuration,
    /// Cadence for physical cleanup of acknowledged deliveries, in human-readable
    /// form (`"5m"`).
    ///
    /// A tick runs only when the tenant may have written since the last
    /// cleanup: a tenant nobody uses and no background pass touches acknowledges
    /// nothing new, and whatever ages past retention meanwhile is removed by the
    /// first cleanup after the tenant next changes.
    #[toml_comment(inline)]
    pub cleanup_interval: HumanDuration,
    /// Age after which a dead-lettered (exhausted) delivery is removed, in
    /// human-readable form (`"7days"`).
    ///
    /// Kept longer than [`Self::cleanup_after`] by default: an exhausted
    /// delivery is retained specifically as evidence of a notification that
    /// never arrived, so it earns a longer look than a delivery that already
    /// succeeded.
    #[toml_comment(inline)]
    pub dead_letter_after: HumanDuration,
    /// Age after which an event with zero referencing deliveries is removed, in
    /// human-readable form (`"1day"`).
    #[toml_comment(inline)]
    pub orphan_event_after: HumanDuration,
}

impl Default for DeliveryConfig {
    fn default() -> DeliveryConfig {
        DeliveryConfig {
            mode: DeliveryMode::Live,
            request_timeout: HumanDuration::from_secs(10),
            max_attempts: 5,
            base_backoff: HumanDuration::from_secs(5),
            poll_interval: HumanDuration::from_secs(1),
            poll_interval_max: HumanDuration::from_secs(30),
            batch_size: 50,
            concurrency: 20,
            lease: HumanDuration::from_secs(30),
            error_max_chars: 1024,
            cleanup_after: HumanDuration::from_secs(86400),
            cleanup_interval: HumanDuration::from_secs(300),
            dead_letter_after: HumanDuration::from_secs(604_800),
            orphan_event_after: HumanDuration::from_secs(86400),
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
    /// dead-letter retention or orphan-event retention, a claim lease term,
    /// stored-diagnostic bound or in-flight delivery bound its own newtype
    /// rejects, or a claim lease term that does not exceed the request timeout
    /// by [`DELIVERY_LEASE_MARGIN`].
    pub fn validate(&self) -> Result<(), DeliveryConfigError> {
        self.attempt_ceiling()?;
        if self.batch_size <= 0 {
            return Err(DeliveryConfigError::NonPositiveBatchSize);
        }
        self.retry_schedule()?;
        if self.poll_interval.is_zero() {
            return Err(DeliveryConfigError::ZeroPollInterval);
        }
        if self.poll_interval_max.duration().as_secs() < self.poll_interval.duration().as_secs() {
            return Err(DeliveryConfigError::PollIntervalCeilingBelowFloor);
        }
        if self.request_timeout.is_zero() {
            return Err(DeliveryConfigError::ZeroRequestTimeout);
        }
        if self.cleanup_after.is_zero() {
            return Err(DeliveryConfigError::ZeroCleanupRetention);
        }
        if self.cleanup_interval.is_zero() {
            return Err(DeliveryConfigError::ZeroCleanupInterval);
        }
        if self.dead_letter_after.is_zero() {
            return Err(DeliveryConfigError::ZeroDeadLetterRetention);
        }
        if self.orphan_event_after.is_zero() {
            return Err(DeliveryConfigError::ZeroOrphanEventRetention);
        }
        self.lease_term()?;
        if self.lease.duration() < self.request_timeout.duration().saturating_add(DELIVERY_LEASE_MARGIN) {
            return Err(DeliveryConfigError::LeaseWithoutMarginOverRequestTimeout {
                margin_seconds: DELIVERY_LEASE_MARGIN.as_secs(),
            });
        }
        self.error_bound()?;
        self.delivery_concurrency()?;
        Ok(())
    }

    /// Whether notifications are recorded and delivered through the durable queue.
    #[must_use]
    pub const fn is_durable(&self) -> bool {
        matches!(self.mode, DeliveryMode::Durable)
    }

    /// The outbound notification request timeout.
    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout.duration()
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

    /// The schedule a failed delivery is retried on: doubling from
    /// `base_backoff` with every failed attempt, and no longer growing once a
    /// delivery reaches its attempt ceiling, where it cannot be claimed again.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryConfigError::AttemptCeiling`] when the attempt ceiling
    /// is not positive, and [`DeliveryConfigError::RetrySchedule`] when
    /// `base_backoff` is zero.
    pub fn retry_schedule(&self) -> Result<BackoffSchedule, DeliveryConfigError> {
        let first = self.base_backoff.duration();
        let ceiling = Growth::Doubling.grow(first, self.attempt_ceiling()?.last_retry());
        BackoffSchedule::validated(first, ceiling, Growth::Doubling).map_err(DeliveryConfigError::RetrySchedule)
    }

    /// The configured claim lease term.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseTermError`] when the configured lease term is zero
    /// seconds or above the longest accepted term.
    pub fn lease_term(&self) -> Result<LeaseTerm, LeaseTermError> {
        let seconds = u32::try_from(self.lease.duration().as_secs()).unwrap_or(u32::MAX);
        LeaseTerm::from_seconds(seconds)
    }

    /// The configured bound on a stored failure diagnostic.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryErrorBoundError`] when the configured bound is zero
    /// characters or above
    /// [`FAILURE_SUMMARY_MAX_CHARACTERS`](andromeda_context_commit::failure_summary::FAILURE_SUMMARY_MAX_CHARACTERS).
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
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;

    fn valid_config() -> DeliveryConfig {
        DeliveryConfig {
            mode: DeliveryMode::Durable,
            request_timeout: HumanDuration::from_secs(10),
            max_attempts: 5,
            base_backoff: HumanDuration::from_secs(5),
            poll_interval: HumanDuration::from_secs(1),
            poll_interval_max: HumanDuration::from_secs(30),
            batch_size: 50,
            concurrency: 20,
            lease: HumanDuration::from_secs(30),
            error_max_chars: 1_024,
            cleanup_after: HumanDuration::from_secs(86_400),
            cleanup_interval: HumanDuration::from_secs(300),
            dead_letter_after: HumanDuration::from_secs(604_800),
            orphan_event_after: HumanDuration::from_secs(86_400),
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
            lease: HumanDuration::from_secs(0),
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
            request_timeout: HumanDuration::from_secs(0),
            ..valid_config()
        };
        assert_eq!(config.validate(), Err(DeliveryConfigError::ZeroRequestTimeout));
    }

    /// A lease that runs out while its delivery is still in flight -- its POST,
    /// or the store round trips around it -- lets a second worker claim and
    /// send the same notification.
    #[test]
    fn a_lease_without_the_margin_over_the_request_timeout_is_rejected() {
        for lease in [5, 10, 11, 14] {
            let config = DeliveryConfig {
                lease: HumanDuration::from_secs(lease),
                request_timeout: HumanDuration::from_secs(10),
                ..valid_config()
            };
            assert_eq!(
                config.validate(),
                Err(DeliveryConfigError::LeaseWithoutMarginOverRequestTimeout { margin_seconds: 5 }),
                "lease {lease}s"
            );
        }
    }

    #[test]
    fn a_lease_covering_the_request_timeout_and_the_margin_is_accepted() {
        let config = DeliveryConfig {
            lease: HumanDuration::from_secs(15),
            request_timeout: HumanDuration::from_secs(10),
            ..valid_config()
        };
        assert_eq!(config.validate(), Ok(()));
    }

    /// A pass claims a delivery only when a slot is free to send it, so a
    /// batch many slots wide needs no longer a lease than a single attempt.
    #[test]
    fn a_batch_many_slots_wide_needs_no_longer_a_lease_than_one_attempt() {
        let config = DeliveryConfig {
            batch_size: 1_000,
            concurrency: 1,
            lease: HumanDuration::from_secs(15),
            request_timeout: HumanDuration::from_secs(10),
            ..valid_config()
        };
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn the_margin_error_names_the_margin() {
        let message = DeliveryConfigError::LeaseWithoutMarginOverRequestTimeout { margin_seconds: 5 }.to_string();
        assert!(message.contains("by at least 5s"), "{message}");
    }

    #[test]
    fn the_shipped_defaults_are_accepted() {
        assert_eq!(DeliveryConfig::default().validate(), Ok(()));
    }

    #[test]
    fn the_delivery_mode_default_is_live() {
        assert_eq!(DeliveryMode::default(), DeliveryMode::Live);
        assert!(!DeliveryConfig::default().is_durable());
    }
}
