//! Operation accounting for subscription lifecycle changes.
//!
//! Distinct from [`crate::subscription`], which records the candidate funnel of
//! one matched event; this module counts the create/update/delete lifecycle
//! operations a client drives.

use crate::{
    names::{LABEL_OPERATION, LABEL_OUTCOME, SUBSCRIPTION_OPERATIONS_TOTAL},
    operation::Outcome,
};
use metrics::counter;
use strum::IntoStaticStr;

/// One subscription lifecycle operation, as it appears in the `operation` label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum SubscriptionOperation {
    /// Create a subscription (clause 5.8.1).
    Create,
    /// Update a subscription (clause 5.8.3).
    Update,
    /// Delete a subscription (clause 5.8.4).
    Delete,
}

/// Count one subscription lifecycle `operation` with its `outcome`.
pub fn record(operation: SubscriptionOperation, outcome: Outcome) {
    counter!(
        SUBSCRIPTION_OPERATIONS_TOTAL,
        LABEL_OPERATION => <&'static str>::from(operation),
        LABEL_OUTCOME => <&'static str>::from(outcome),
    )
    .increment(1);
}

/// Count one subscription lifecycle `operation` with the outcome of `result`.
pub fn record_result<T, E>(operation: SubscriptionOperation, result: &Result<T, E>) {
    record(operation, Outcome::of(result));
}
