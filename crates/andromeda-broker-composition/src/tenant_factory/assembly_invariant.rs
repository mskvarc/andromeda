//! What a tenant runtime cannot be assembled from, when the configuration that
//! validated at startup still does not describe a runtime the parts accept.
//!
//! Every variant carries the refusal of the part that was being constructed, so
//! an operator reads which part refused and why, not a flattened sentence.

use andromeda_configuration::schema::temporal::storage::StorageDaysError;
use andromeda_delivery::error::TransportError;
use andromeda_entity::error::EntityError;
use andromeda_federation::error::FederationError;
use andromeda_notification::error::NotificationError;
use andromeda_postgresql_temporal_store::retention_sweep::TemporalRetentionHorizonInvalid;
use andromeda_retry::schedule_error::BackoffScheduleError;
use andromeda_snapshot::error::SnapshotError;
use andromeda_task_runtime::{periodic_trigger::PeriodicScheduleError, registration::TaskRegistrationError, task_name::TaskNameError};
use thiserror::Error;

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
    /// How long a side-effect job retired for an unreadable payload is kept.
    #[display("pending side-effect dead-letter age")]
    SideEffectDeadLetter,
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
    DrainFallback(#[from] BackoffScheduleError),
    /// The task supervisor refused a registration.
    #[error("the task supervisor refused a registration: {0}")]
    Registration(#[from] TaskRegistrationError),
    /// A retention age is past the range a signed duration represents.
    #[error("{0} is out of range")]
    RetentionOutOfRange(RetentionSetting),
    /// The temporal intent retry schedule the configuration names is not
    /// valid.
    #[error("the temporal intent retry policy is not valid: {0}")]
    IntentRetry(#[source] BackoffScheduleError),
    /// The pending side-effect retry schedule the configuration names is not
    /// valid.
    #[error("the pending side-effect retry schedule is not valid: {0}")]
    SideEffectRetry(#[source] BackoffScheduleError),
    /// The temporal storage policy the configuration names is not valid.
    #[error("the temporal storage policy is not valid: {0}")]
    StoragePolicy(#[from] StorageDaysError),
    /// The temporal retention horizon is not one the sweep accepts.
    #[error("the temporal retention horizon is not valid: {0}")]
    RetentionHorizon(#[from] TemporalRetentionHorizonInvalid),
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
    use crate::tenant_factory::assembly_invariant::{AssemblyInvariant, RetentionSetting};

    #[test]
    fn an_out_of_range_retention_names_the_setting() {
        pretty_assertions::assert_eq!(
            AssemblyInvariant::RetentionOutOfRange(RetentionSetting::NotificationOrphanEvent).to_string(),
            "notification delivery orphan-event age is out of range"
        );
    }
}
