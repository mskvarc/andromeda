//! What notification delivery takes from the `[notifications.delivery]` section.

use crate::{
    delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError},
    delivery_concurrency::{DeliveryConcurrency, DeliveryConcurrencyError},
    delivery_error_bound::{DeliveryErrorBound, DeliveryErrorBoundError},
    delivery_lease::{LeaseTerm, LeaseTermError},
};
use andromeda_configuration::schema::delivery::DeliveryConfig;
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use thiserror::Error;

/// A delivery value the configuration names that the delivery layer cannot use.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryPolicyError {
    /// The attempt ceiling was outside the accepted range.
    #[error("notifications.delivery.max_attempts is invalid: {0}")]
    AttemptCeiling(#[from] DeliveryAttemptCeilingError),
    /// The retry backoff does not bound a retry schedule.
    #[error("notifications.delivery.base_backoff does not form a retry schedule: {0}")]
    RetrySchedule(#[source] BackoffScheduleError),
    /// The claim lease term was outside the accepted range.
    #[error("notifications.delivery.lease is invalid: {0}")]
    LeaseTerm(#[from] LeaseTermError),
    /// The stored-diagnostic bound was outside the accepted range.
    #[error("notifications.delivery.error_max_chars is invalid: {0}")]
    ErrorBound(#[from] DeliveryErrorBoundError),
    /// The in-flight delivery bound was outside the accepted range.
    #[error("notifications.delivery.concurrency is invalid: {0}")]
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
    DeliveryConcurrency::from_count(config.concurrency)
}

/// The configured attempt ceiling.
///
/// # Errors
///
/// Returns [`DeliveryAttemptCeilingError::NonPositiveAttempts`] when the
/// configured attempt ceiling is zero or negative.
pub const fn attempt_ceiling(config: &DeliveryConfig) -> Result<DeliveryAttemptCeiling, DeliveryAttemptCeilingError> {
    DeliveryAttemptCeiling::from_attempts(config.max_attempts)
}

/// The schedule a failed delivery is retried on: doubling from
/// `base_backoff` with every failed attempt, and no longer growing once a
/// delivery reaches its attempt ceiling, where it cannot be claimed again.
///
/// # Errors
///
/// Returns [`DeliveryPolicyError::AttemptCeiling`] when the attempt ceiling
/// is not positive, and [`DeliveryPolicyError::RetrySchedule`] when
/// `base_backoff` is zero.
pub fn retry_schedule(config: &DeliveryConfig) -> Result<BackoffSchedule, DeliveryPolicyError> {
    let first = config.base_backoff.duration();
    let ceiling = Growth::Doubling.grow(first, attempt_ceiling(config)?.last_retry());
    BackoffSchedule::validated(first, ceiling, Growth::Doubling).map_err(DeliveryPolicyError::RetrySchedule)
}

/// The configured claim lease term.
///
/// # Errors
///
/// Returns [`LeaseTermError`] when the configured lease term is zero
/// seconds or above the longest accepted term.
pub fn lease_term(config: &DeliveryConfig) -> Result<LeaseTerm, LeaseTermError> {
    let seconds = u32::try_from(config.lease.duration().as_secs()).unwrap_or(u32::MAX);
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
    DeliveryErrorBound::from_characters(config.error_max_chars)
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
    use andromeda_configuration::{human_duration::HumanDuration, schema::delivery::DeliveryConfig};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_shipped_delivery_values_are_usable() {
        assert_eq!(check_delivery_policy(&DeliveryConfig::default()), Ok(()));
    }

    #[test]
    fn a_non_positive_attempt_ceiling_is_rejected() {
        let config = DeliveryConfig {
            max_attempts: 0,
            ..DeliveryConfig::default()
        };
        assert_eq!(
            check_delivery_policy(&config),
            Err(DeliveryPolicyError::AttemptCeiling(DeliveryAttemptCeilingError::NonPositiveAttempts))
        );
    }

    #[test]
    fn zero_lease_term_is_rejected() {
        let config = DeliveryConfig {
            lease: HumanDuration::from_secs(0),
            ..DeliveryConfig::default()
        };
        assert_eq!(check_delivery_policy(&config), Err(DeliveryPolicyError::LeaseTerm(LeaseTermError::ZeroSeconds)));
    }

    #[test]
    fn an_error_bound_above_the_schema_maximum_is_rejected() {
        let config = DeliveryConfig {
            error_max_chars: 4_096,
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
    fn zero_delivery_concurrency_is_rejected() {
        let config = DeliveryConfig {
            concurrency: 0,
            ..DeliveryConfig::default()
        };
        assert_eq!(
            check_delivery_policy(&config),
            Err(DeliveryPolicyError::Concurrency(DeliveryConcurrencyError::Zero))
        );
    }
}
