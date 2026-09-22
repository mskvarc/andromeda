//! Snapshot background task registration: preparation, wake-driven status
//! delivery, status-outbox retention cleanup, and the `expiresAt` sweep.

use crate::{
    event::snapshot_notification_sink::{DeliverySnapshotNotificationSink, SnapshotStatusDeliveryDrain},
    tenant_factory::error::TenantAssemblyError,
};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_notification::delivery_config::DeliveryConfig;
use andromeda_postgresql_context_store::snapshot::status_outbox::SnapshotStatusOutbox;
use andromeda_snapshot::{
    error::SnapshotError,
    executor::{SnapshotPreparation, SnapshotPreparationOutcome},
    expiry::SnapshotExpiry,
    status_delivery_signal::StatusDeliverySignal,
    store::SnapshotStore,
};
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::TaskFailure,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver, WakeHandle, WakeRequest},
};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};

/// Register snapshot preparation and the durable delivery of the lifecycle
/// notifications it produces.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configured delivery cadence is
/// rejected or when the supervisor refuses either registration.
/// The wake endpoints the Snapshot status-delivery workers share.
///
/// The scheduled sweep and the create-time preparation path both wake through
/// `sweep_wake`; the delivery drain reads `receiver`. Both are created by the
/// tenant assembly so the create-time path can hold its own clone of the handle
/// while the drain is still being registered.
pub(super) struct SnapshotStatusWake {
    /// Handle the scheduled sweep and create-time preparation wake through.
    pub(super) sweep_wake: WakeHandle,
    /// The delivery drain's wake receiver.
    pub(super) receiver: DrainWakeReceiver,
}

pub(super) fn register_snapshot_workers(
    supervisor: &TaskSupervisor,
    pool: &PgPool,
    preparation: Arc<SnapshotPreparation>,
    transport: &Arc<TransportDispatcher>,
    config: &DeliveryConfig,
    preparation_recovery_secs: u64,
    wake: SnapshotStatusWake,
) -> Result<(), TenantAssemblyError> {
    // The scheduled sweep wakes status delivery the moment it prepares a
    // Snapshot, so the delivery drain's idle interval can widen without adding
    // latency. The create-time preparation path shares the same channel through
    // its own clone of the handle, so a freshly created Snapshot's notification
    // does not wait for the drain's fallback either.
    let SnapshotStatusWake { sweep_wake, receiver } = wake;
    register_snapshot_preparation(supervisor, preparation, sweep_wake, Duration::from_secs(preparation_recovery_secs))?;
    let drain = Arc::new(
        SnapshotStatusDeliveryDrain::new(
            Arc::new(SnapshotStatusOutbox::new(pool)),
            Arc::new(DeliverySnapshotNotificationSink::new(Arc::clone(transport))), // clone: every worker delivers through one dispatcher.
            config,
        )
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?,
    );
    let floor = Duration::from_secs(config.poll_interval_secs.max(1));
    let ceiling = Duration::from_secs(config.poll_interval_max_secs.max(config.poll_interval_secs).max(1));
    register_snapshot_status_delivery(supervisor, Arc::clone(&drain), floor, ceiling, receiver)?; // clone: delivery and cleanup share one drain.
    register_snapshot_status_cleanup(supervisor, drain, Duration::from_secs(config.cleanup_interval_secs.max(1)))
}

/// Register the sweep that removes Snapshots past their `expiresAt`.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the cadence is rejected or when the
/// supervisor refuses the registration.
pub(super) fn register_snapshot_expiry(
    supervisor: &TaskSupervisor,
    store: Arc<dyn SnapshotStore>,
    expiration_interval: u64,
) -> Result<(), TenantAssemblyError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("snapshot-expiry").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(SnapshotExpiry::new(store));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each execution owns the bounded operation while running.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff());
            let expired = expiry.execute(cutoff).await.map_err(|error| classify_snapshot_failure(&error))?;
            tracing::info!(expired, "snapshot expiry completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn register_snapshot_preparation(
    supervisor: &TaskSupervisor,
    preparation: Arc<SnapshotPreparation>,
    status_wake: WakeHandle,
    cadence: Duration,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("snapshot-preparation").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let preparation = Arc::clone(&preparation); // clone: each execution owns the bounded operation while running.
        let status_wake = status_wake.clone(); // clone: each execution wakes delivery through its own handle.
        async move {
            match preparation.execute().await.map_err(|error| classify_snapshot_failure(&error))? {
                SnapshotPreparationOutcome::Idle => {}
                SnapshotPreparationOutcome::Prepared { count } => {
                    tracing::info!(count, "snapshot preparation completed");
                    // A prepared Snapshot enqueued its status notification, so
                    // deliver it now rather than on the delivery drain's idle
                    // fallback.
                    report_status_wake(status_wake.wake());
                }
            }
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Wakes the Snapshot status-delivery drain from the service paths that enqueue
/// notifications outside the scheduled preparation sweep: a create-time
/// preparation and a metadata update.
pub(crate) struct StatusDeliveryWaker {
    /// A clone of the drain's wake handle, shared with the scheduled sweep.
    wake: WakeHandle,
}

impl StatusDeliveryWaker {
    /// Wrap a drain wake handle as the service's delivery signal.
    pub(crate) const fn new(wake: WakeHandle) -> StatusDeliveryWaker {
        StatusDeliveryWaker { wake }
    }
}

impl StatusDeliverySignal for StatusDeliveryWaker {
    fn signal(&self) {
        report_status_wake(self.wake.wake());
    }
}

/// Log a status-delivery wake that reached a stopped drain; a queued or
/// coalesced wake is silent.
fn report_status_wake(request: WakeRequest) {
    match request {
        WakeRequest::Scheduled | WakeRequest::Coalesced => (),
        WakeRequest::Closed => tracing::warn!(
            event = "snapshot_status_wake_closed",
            "the snapshot status delivery drain no longer receives wakes"
        ),
    }
}

/// Register durable Snapshot lifecycle notification delivery as a wake-driven drain.
fn register_snapshot_status_delivery(
    supervisor: &TaskSupervisor,
    drain: Arc<SnapshotStatusDeliveryDrain>,
    floor: Duration,
    ceiling: Duration,
    wake: DrainWakeReceiver,
) -> Result<(), TenantAssemblyError> {
    let fallback = DrainFallbackPolicy::new(floor, ceiling).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("snapshot-status-delivery").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, floor, ceiling),
        ShutdownPolicy::new(floor),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let drain = Arc::clone(&drain); // clone: each bounded pass owns the shared delivery operation while executing.
        async move {
            let delivered = drain
                .execute(DateTime::<Utc>::from(execution.cutoff()))
                .await
                .map_err(|error| classify_snapshot_failure(&error))?;
            if delivered > 0 {
                tracing::info!(claimed = delivered, "snapshot status notification delivery completed");
            }
            // A full batch may leave more waiting, so drain again immediately.
            if i32::try_from(delivered).is_ok_and(|claimed| claimed >= drain.batch_size()) {
                Ok(DrainPassOutcome::WorkMayRemain)
            } else {
                Ok(DrainPassOutcome::Idle)
            }
        }
    });
    supervisor
        .register_drain(task, wake)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Register the retention cleanup of the Snapshot status outbox on its own cadence.
fn register_snapshot_status_cleanup(
    supervisor: &TaskSupervisor,
    drain: Arc<SnapshotStatusDeliveryDrain>,
    cadence: Duration,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("snapshot-status-cleanup").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let drain = Arc::clone(&drain); // clone: each cleanup pass owns the shared delivery operation while executing.
        async move {
            drain
                .cleanup(DateTime::<Utc>::from(execution.cutoff()))
                .await
                .map_err(|error| classify_snapshot_failure(&error))?;
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Classify a snapshot policy failure at the task boundary.
fn classify_snapshot_failure(error: &SnapshotError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        SnapshotError::Persistence { .. } | SnapshotError::StoreSaturated | SnapshotError::Contended => TaskFailure::transient(summary),
        SnapshotError::Internal { .. } => TaskFailure::invariant_violation(summary),
        SnapshotError::Validation { .. }
        | SnapshotError::NotFound { .. }
        | SnapshotError::AlreadyExists { .. }
        | SnapshotError::PreparationTimeout { .. }
        | SnapshotError::Entity(_)
        | SnapshotError::Temporal(_)
        | SnapshotError::Federation(_)
        | SnapshotError::Request(_)
        | SnapshotError::Representation(_) => TaskFailure::permanent(summary),
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::snapshot::StatusDeliveryWaker;
    use andromeda_snapshot::status_delivery_signal::StatusDeliverySignal;
    use andromeda_task_runtime::wake_trigger::{WakeRequest, wake_channel};

    /// The create-time and update paths reach delivery only through this signal,
    /// so raising it must place a wake on the drain's one-slot channel. A probe
    /// handle sharing that channel then coalesces rather than schedules, which it
    /// could only do if the signal had already filled the slot.
    #[test]
    fn signalling_the_delivery_waker_places_a_wake_on_the_drain_channel() {
        let (handle, _receiver) = wake_channel();
        let probe = handle.clone(); // clone: a second producer observes the channel state the signal leaves.
        let waker = StatusDeliveryWaker::new(handle);
        waker.signal();
        assert_eq!(
            probe.wake(),
            WakeRequest::Coalesced,
            "the delivery signal must have filled the drain's one-slot wake channel"
        );
    }
}
