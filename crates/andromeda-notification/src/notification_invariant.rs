//! The invariants notification delivery relies on, and how each one broke.

use crate::{
    delivery_attempt_ceiling::DeliveryAttemptCeilingError,
    delivery_concurrency::DeliveryConcurrencyError,
    delivery_error_bound::DeliveryErrorBoundError,
    delivery_lease::LeaseTermError,
    delivery_policy::DeliveryPolicyError,
};
use andromeda_retry::retry_number::RetryNumberError;
use std::num::TryFromIntError;
use thiserror::Error;

/// A notification delivery invariant that did not hold.
///
/// Each is a property of the delivery configuration the broker validated at
/// startup or of a width the drain computed itself, so a broken one is a fault
/// of the broker, not of a subscription.
#[derive(Debug, Error)]
pub enum NotificationInvariant {
    /// The configured lease term is unusable.
    #[error("the notification delivery lease term is unusable: {0}")]
    LeaseTerm(#[from] LeaseTermError),
    /// The configured delivery error bound is unusable.
    #[error("the notification delivery error bound is unusable: {0}")]
    ErrorBound(#[from] DeliveryErrorBoundError),
    /// The configured delivery concurrency is unusable.
    #[error("the notification delivery concurrency is unusable: {0}")]
    Concurrency(#[from] DeliveryConcurrencyError),
    /// The configured attempt ceiling is unusable.
    #[error("the notification delivery attempt ceiling is unusable: {0}")]
    AttemptCeiling(#[from] DeliveryAttemptCeilingError),
    /// The configured batch size does not fit the platform address width.
    #[error("the notification delivery batch size does not fit the platform address width")]
    BatchSizeExceedsAddressWidth,
    /// A claim width the drain computed does not fit the store's claim bound.
    #[error("the notification delivery claim width is out of range: {0}")]
    ClaimWidth(#[source] TryFromIntError),
    /// The configured retry schedule is unusable.
    #[error("the notification delivery retry schedule is unusable: {0}")]
    RetrySchedule(#[source] DeliveryPolicyError),
    /// A claimed delivery records a negative attempt count.
    #[error("a claimed notification delivery records no attempt count: {0}")]
    AttemptCount(#[source] RetryNumberError),
}
