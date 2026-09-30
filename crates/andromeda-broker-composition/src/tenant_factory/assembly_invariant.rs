//! What a tenant runtime cannot be assembled from, when the configuration that
//! validated at startup still does not describe a runtime the parts accept.
//!
//! Every variant carries the refusal of the part that was being constructed, so
//! an operator reads which part refused and why, not a flattened sentence.

use andromeda_delivery::error::TransportError;
use andromeda_entity::{error::EntityError, expiry::EntityExpiryBatchSizeError};
use andromeda_federation::error::FederationError;
use andromeda_notification::error::NotificationError;
use andromeda_postgresql_temporal_store::retention_sweep::TemporalRetentionHorizonInvalid;
use andromeda_snapshot::error::SnapshotError;
use andromeda_task_runtime::{
    periodic_trigger::PeriodicScheduleError,
    registration::TaskRegistrationError,
    task_name::TaskNameError,
    wake_trigger::DrainFallbackPolicyError,
};
use andromeda_temporal::{config::TemporalConfigError, intent_retry_policy::RetryCeilingBelowBase};
use thiserror::Error;

/// A setting that must name a positive count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub(super) enum CountSetting {
    /// How many intents one temporal delivery pass claims.
    #[display("temporal.intent_batch_size")]
    TemporalIntentBatchSize,
    /// How many failed attempts mark a temporal intent stuck.
    #[display("temporal.intent_stuck_after_attempts")]
    TemporalIntentStuckAfterAttempts,
}

/// A retention age that is subtracted from an instant as a signed duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub(super) enum RetentionSetting {
    /// How long a delivered notification is kept.
    #[display("notification delivery cleanup age")]
    NotificationCleanup,
    /// How long a dead-lettered notification is kept.
    #[display("notification delivery dead-letter age")]
    NotificationDeadLetter,
    /// How long an event no delivery references is kept.
    #[display("notification delivery orphan-event age")]
    NotificationOrphanEvent,
    /// How long a dead-lettered side-effect job is kept.
    #[display("pending side-effect dead-letter age")]
    SideEffectDeadLetter,
    /// How far back temporal history is retained.
    #[display("temporal retention horizon")]
    TemporalHorizon,
}

/// A part of a tenant runtime that refused to be constructed.
#[derive(Debug, Error)]
pub(super) enum AssemblyInvariant {
    /// A background task's name is not a task name.
    #[error("a background task name is not valid: {0}")]
    TaskName(#[from] TaskNameError),
    /// A background task's cadence names no usable schedule.
    #[error("a background task schedule is not valid: {0}")]
    Schedule(#[from] PeriodicScheduleError),
    /// A drain's fallback interval names no usable policy.
    #[error("a drain fallback policy is not valid: {0}")]
    DrainFallback(#[from] DrainFallbackPolicyError),
    /// The task supervisor refused a registration.
    #[error("the task supervisor refused a registration: {0}")]
    Registration(#[from] TaskRegistrationError),
    /// The temporal intent retry ceiling is below its base.
    #[error("the temporal intent retry policy is not valid: {0}")]
    IntentRetry(#[from] RetryCeilingBelowBase),
    /// A count setting is zero.
    #[error("{0} must be non-zero")]
    ZeroCount(CountSetting),
    /// A retention age is past the range a signed duration represents.
    #[error("{0} is out of range")]
    RetentionOutOfRange(RetentionSetting),
    /// The temporal storage policy the configuration names is not valid.
    #[error("the temporal storage policy is not valid: {0}")]
    StoragePolicy(#[from] TemporalConfigError),
    /// The temporal retention horizon is not one the sweep accepts.
    #[error("the temporal retention horizon is not valid: {0}")]
    RetentionHorizon(#[from] TemporalRetentionHorizonInvalid),
    /// The entity expiry batch size is not one the sweep accepts.
    #[error("the entity expiry batch size is not valid: {0}")]
    ExpiryBatchSize(#[from] EntityExpiryBatchSizeError),
    /// The notification transport dispatcher could not be created.
    #[error("the transport dispatcher could not be created: {0}")]
    TransportDispatcher(#[from] TransportError),
    /// The federation service could not be created.
    #[error("the federation service could not be created: {0}")]
    Federation(#[source] Box<FederationError>),
    /// The entity location placement could not be created.
    #[error("the entity location placement could not be created: {0}")]
    LocationPlacement(#[source] Box<EntityError>),
    /// The snapshot status delivery drain could not be created.
    #[error("the snapshot status delivery drain could not be created: {0}")]
    SnapshotStatusDrain(#[source] Box<SnapshotError>),
    /// The notification delivery drain could not be created.
    #[error("the notification delivery drain could not be created: {0}")]
    NotificationDrain(#[source] Box<NotificationError>),
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::assembly_invariant::{AssemblyInvariant, CountSetting, RetentionSetting};

    #[test]
    fn a_zero_count_names_the_configuration_key() {
        pretty_assertions::assert_eq!(
            AssemblyInvariant::ZeroCount(CountSetting::TemporalIntentBatchSize).to_string(),
            "temporal.intent_batch_size must be non-zero"
        );
    }

    #[test]
    fn an_out_of_range_retention_names_the_setting() {
        pretty_assertions::assert_eq!(
            AssemblyInvariant::RetentionOutOfRange(RetentionSetting::NotificationOrphanEvent).to_string(),
            "notification delivery orphan-event age is out of range"
        );
    }
}
