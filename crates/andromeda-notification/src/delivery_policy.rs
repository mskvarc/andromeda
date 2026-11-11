//! What notification delivery takes from the `[notifications.delivery]` section.

use crate::{
    delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError},
    delivery_backoff::{DeliveryBackoff, DeliveryBackoffError},
    delivery_concurrency::{DeliveryConcurrency, DeliveryConcurrencyError},
    delivery_error_bound::{DeliveryErrorBound, DeliveryErrorBoundError},
    delivery_lease::{LeaseTerm, LeaseTermError},
};
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use thiserror::Error;

/// The retry delays of `[notifications.delivery.queue]` do not form a retry
/// schedule the delivery queue can wait on.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryPolicyError {
    /// The retry delays do not bound a retry schedule.
    #[error("notifications.delivery.queue.retry_interval and max_retry_interval do not form a retry schedule: {0}")]
    RetrySchedule(#[source] BackoffScheduleError),
    /// The longest retry delay is longer than the queue waits.
    #[error("notifications.delivery.queue.max_retry_interval is longer than a delivery waits: {0}")]
    RetryStep(#[source] DeliveryBackoffError),
}

/// The configured in-flight delivery bound.
///
/// # Errors
///
/// Returns [`DeliveryConcurrencyError`] when the configured concurrency is
/// zero or above [`DeliveryConcurrency::MAXIMUM`].
pub const fn delivery_concurrency(config: &DeliveryConfig) -> Result<DeliveryConcurrency, DeliveryConcurrencyError> {
    DeliveryConcurrency::from_count(config.max_concurrent.get())
}

/// The configured attempt ceiling.
///
/// # Errors
///
/// Returns [`DeliveryAttemptCeilingError::AboveStoredRange`] when the
/// configured attempt ceiling exceeds what the store counts.
pub fn attempt_ceiling(config: &DeliveryConfig) -> Result<DeliveryAttemptCeiling, DeliveryAttemptCeilingError> {
    DeliveryAttemptCeiling::from_attempts(config.max_attempts)
}

/// The schedule a failed delivery is retried on: doubling from
/// `retry_interval` with every failed attempt, up to `max_retry_interval`.
///
/// # Errors
///
/// Returns [`DeliveryPolicyError::RetrySchedule`] when `retry_interval` is
/// zero or `max_retry_interval` below it, and
/// [`DeliveryPolicyError::RetryStep`] when `max_retry_interval` exceeds
/// [`DeliveryBackoff::MAX_STEP`].
pub const fn retry_schedule(config: &DeliveryConfig) -> Result<DeliveryBackoff, DeliveryPolicyError> {
    match BackoffSchedule::validated(
        config.queue.retry_interval.duration(),
        config.queue.max_retry_interval.duration(),
        Growth::Doubling,
    ) {
        Ok(schedule) => match DeliveryBackoff::new(schedule) {
            Ok(backoff) => Ok(backoff),
            Err(error) => Err(DeliveryPolicyError::RetryStep(error)),
        },
        Err(error) => Err(DeliveryPolicyError::RetrySchedule(error)),
    }
}

/// The configured claim lease term.
///
/// # Errors
///
/// Returns [`LeaseTermError`] when the configured lease term is zero
/// seconds or above the longest accepted term.
pub fn lease_term(config: &DeliveryConfig) -> Result<LeaseTerm, LeaseTermError> {
    let seconds = u32::try_from(config.queue.lease_timeout.duration().as_secs()).unwrap_or(u32::MAX);
    LeaseTerm::from_seconds(seconds)
}

/// The configured bound on a stored failure diagnostic.
///
/// # Errors
///
/// Returns [`DeliveryErrorBoundError`] when the configured bound is zero
/// characters or above
/// [`FAILURE_SUMMARY_MAX_CHARACTERS`](andromeda_context_commit::failure_summary::FAILURE_SUMMARY_MAX_CHARACTERS).
pub const fn error_bound(config: &DeliveryConfig) -> Result<DeliveryErrorBound, DeliveryErrorBoundError> {
    DeliveryErrorBound::from_characters(config.max_error_length.get())
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_attempt_ceiling::DeliveryAttemptCeiling,
        delivery_concurrency::DeliveryConcurrency,
        delivery_error_bound::DeliveryErrorBound,
        delivery_lease::LeaseTerm,
        delivery_policy::{attempt_ceiling, delivery_concurrency, error_bound, lease_term, retry_schedule},
    };
    use andromeda_configuration::{
        human_duration::HumanDuration,
        schema::notifications::delivery::{
            DeliveryConfig,
            MAX_CONCURRENT_DELIVERIES_CEILING,
            MAX_DELIVERY_ATTEMPTS_CEILING,
            MAX_DELIVERY_LEASE,
            MAX_DELIVERY_RETRY_INTERVAL,
            MAX_ERROR_LENGTH_CEILING,
        },
    };
    use std::{num::NonZeroU32, time::Duration};

    /// The shipped delivery values with `max_retry_interval` at `ceiling`.
    fn retrying_up_to(ceiling: Duration) -> DeliveryConfig {
        let mut config = DeliveryConfig::default();
        config.queue.max_retry_interval = HumanDuration::new(ceiling);
        config
    }

    #[test]
    fn the_shipped_delivery_values_are_usable() {
        let config = DeliveryConfig::default();
        assert!(attempt_ceiling(&config).is_ok());
        assert!(retry_schedule(&config).is_ok());
        assert!(lease_term(&config).is_ok());
        assert!(error_bound(&config).is_ok());
        assert!(delivery_concurrency(&config).is_ok());
    }

    /// The configuration refuses a delivery value past its declared ceiling,
    /// so every value it admits must be one the delivery layer can hold.
    #[test]
    fn every_delivery_ceiling_the_configuration_declares_is_one_the_delivery_layer_holds() {
        assert!(DeliveryConcurrency::from_count(MAX_CONCURRENT_DELIVERIES_CEILING).is_ok());
        assert!(DeliveryErrorBound::from_characters(MAX_ERROR_LENGTH_CEILING).is_ok());
        assert!(NonZeroU32::new(MAX_DELIVERY_ATTEMPTS_CEILING).is_some_and(|attempts| DeliveryAttemptCeiling::from_attempts(attempts).is_ok()));
        assert!(u32::try_from(MAX_DELIVERY_LEASE.as_secs()).is_ok_and(|seconds| LeaseTerm::from_seconds(seconds).is_ok()));
        assert!(retry_schedule(&retrying_up_to(MAX_DELIVERY_RETRY_INTERVAL)).is_ok());
    }

    /// Pinned from above too, so a ceiling the delivery layer raises is
    /// raised in the configuration it is declared in.
    #[test]
    fn every_delivery_ceiling_the_configuration_declares_is_the_delivery_layers_own() {
        assert!(DeliveryConcurrency::from_count(MAX_CONCURRENT_DELIVERIES_CEILING.saturating_add(1)).is_err());
        assert!(DeliveryErrorBound::from_characters(MAX_ERROR_LENGTH_CEILING.saturating_add(1)).is_err());
        assert!(
            NonZeroU32::new(MAX_DELIVERY_ATTEMPTS_CEILING.saturating_add(1)).is_some_and(|attempts| DeliveryAttemptCeiling::from_attempts(attempts).is_err())
        );
        assert!(u32::try_from(MAX_DELIVERY_LEASE.as_secs().saturating_add(1)).is_ok_and(|seconds| LeaseTerm::from_seconds(seconds).is_err()));
        assert!(retry_schedule(&retrying_up_to(MAX_DELIVERY_RETRY_INTERVAL + Duration::from_nanos(1))).is_err());
    }
}
