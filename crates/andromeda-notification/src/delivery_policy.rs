//! What notification delivery takes from the `[notifications.delivery]` section.

use crate::{
    delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError},
    delivery_concurrency::{DeliveryConcurrency, DeliveryConcurrencyError},
    delivery_error_bound::{DeliveryErrorBound, DeliveryErrorBoundError},
    delivery_lease::{LeaseTerm, LeaseTermError},
};
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use thiserror::Error;

/// A delivery value the configuration names that the delivery layer cannot use.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryPolicyError {
    /// The attempt ceiling was outside the accepted range.
    #[error("notifications.delivery.queue.max_attempts is invalid: {0}")]
    AttemptCeiling(#[from] DeliveryAttemptCeilingError),
    /// The retry delays do not bound a retry schedule.
    #[error("notifications.delivery.queue.retry_interval and max_retry_interval do not form a retry schedule: {0}")]
    RetrySchedule(#[source] BackoffScheduleError),
    /// The claim lease term was outside the accepted range.
    #[error("notifications.delivery.queue.lease_timeout is invalid: {0}")]
    LeaseTerm(#[from] LeaseTermError),
    /// The stored-diagnostic bound was outside the accepted range.
    #[error("notifications.delivery.max_error_length is invalid: {0}")]
    ErrorBound(#[from] DeliveryErrorBoundError),
    /// The in-flight delivery bound was outside the accepted range.
    #[error("notifications.delivery.max_concurrent is invalid: {0}")]
    Concurrency(#[from] DeliveryConcurrencyError),
}

/// Read every delivery value the section names.
///
/// # Errors
///
/// Returns the [`DeliveryPolicyError`] of the first value the delivery layer
/// cannot use; the configuration loader asks, so a broker never starts with one.
pub fn check_delivery_policy(config: &DeliveryConfig) -> Result<(), DeliveryPolicyError> {
    attempt_ceiling(config)?;
    retry_schedule(config)?;
    lease_term(config)?;
    error_bound(config)?;
    delivery_concurrency(config)?;
    Ok(())
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
    DeliveryAttemptCeiling::from_attempts(config.queue.max_attempts)
}

/// The schedule a failed delivery is retried on: doubling from
/// `retry_interval` with every failed attempt, up to `max_retry_interval`.
///
/// # Errors
///
/// Returns [`DeliveryPolicyError::RetrySchedule`] when `retry_interval` is
/// zero or `max_retry_interval` below it.
pub const fn retry_schedule(config: &DeliveryConfig) -> Result<BackoffSchedule, DeliveryPolicyError> {
    match BackoffSchedule::validated(
        config.queue.retry_interval.duration(),
        config.queue.max_retry_interval.duration(),
        Growth::Doubling,
    ) {
        Ok(schedule) => Ok(schedule),
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
        delivery_attempt_ceiling::DeliveryAttemptCeilingError,
        delivery_concurrency::DeliveryConcurrencyError,
        delivery_error_bound::DeliveryErrorBoundError,
        delivery_lease::LeaseTermError,
        delivery_policy::{DeliveryPolicyError, check_delivery_policy},
    };
    use andromeda_configuration::{human_duration::HumanDuration, schema::notifications::delivery::DeliveryConfig};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    #[test]
    fn the_shipped_delivery_values_are_usable() {
        assert_eq!(check_delivery_policy(&DeliveryConfig::default()), Ok(()));
    }

    #[test]
    fn an_attempt_ceiling_past_the_stored_range_is_rejected() {
        let mut config = DeliveryConfig::default();
        config.queue.max_attempts = NonZeroU32::MAX;
        assert_eq!(
            check_delivery_policy(&config),
            Err(DeliveryPolicyError::AttemptCeiling(DeliveryAttemptCeilingError::AboveStoredRange {
                attempts: u32::MAX,
                maximum: i32::MAX,
            }))
        );
    }

    #[test]
    fn zero_lease_term_is_rejected() {
        let mut config = DeliveryConfig::default();
        config.queue.lease_timeout = HumanDuration::from_secs(0);
        assert_eq!(check_delivery_policy(&config), Err(DeliveryPolicyError::LeaseTerm(LeaseTermError::ZeroSeconds)));
    }

    #[test]
    fn an_error_bound_above_the_schema_maximum_is_rejected() {
        let config = DeliveryConfig {
            max_error_length: NonZeroU32::MIN.saturating_add(4_095),
            ..DeliveryConfig::default()
        };
        assert_eq!(
            check_delivery_policy(&config),
            Err(DeliveryPolicyError::ErrorBound(DeliveryErrorBoundError::AboveSchemaMaximum {
                characters: 4_096,
                maximum: 2_048,
            }))
        );
    }

    #[test]
    fn a_delivery_concurrency_above_the_maximum_is_rejected() {
        let config = DeliveryConfig {
            max_concurrent: NonZeroU32::MIN.saturating_add(1_024),
            ..DeliveryConfig::default()
        };
        assert_eq!(
            check_delivery_policy(&config),
            Err(DeliveryPolicyError::Concurrency(DeliveryConcurrencyError::TooHigh {
                concurrency: 1_025,
                maximum: 1_024,
            }))
        );
    }
}
