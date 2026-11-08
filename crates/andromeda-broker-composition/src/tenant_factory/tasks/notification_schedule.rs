//! Registering a periodic notification evaluation, and classifying what one
//! reports when it fails.
//!
//! Entity notifications and context source notifications are different
//! policies with different operations, but both are swept on a fixed cadence
//! by a task the supervisor owns, and both report the same
//! [`NotificationError`]. Registering the schedule and reading that error are
//! the two things they genuinely share.

use crate::tenant_factory::{
    assembly_invariant::AssemblyInvariant,
    error::TenantAssemblyError,
    periodic_due::PeriodicSweep,
    tasks::{
        PeriodicPacing,
        notification_worker_failure::{ChangeChannel, channel_closed},
    },
};
use andromeda_ngsi_ld_compaction::compaction_error::CompactionError;
use andromeda_notification::error::NotificationError;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::{FailureClass, TaskFailure},
    registration::TaskRegistration,
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver, WakeHandle},
};
use chrono::{DateTime, Utc};
use std::{
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::{broadcast, broadcast::error::RecvError};

/// Register `operation` as the wake-driven periodic sweep `sweep`, named
/// `task_name`.
///
/// The operation runs one sweep at `cutoff` and reports the earliest instant a
/// subscription is next due, or `None` when none is. The drain then sleeps
/// exactly until that instant, however soon it falls, waking early on `wake`
/// and never sleeping past the pacing's ceiling: clause 5.8.6 notifies when
/// the interval is reached, and Table 5.2.12-1 admits any `timeInterval`
/// greater than 0, so a grid point is never rounded up to a minimum sleep.
/// Every reported instant is also published, so the runtime cache keeps the
/// tenant loaded while a notification falls due shortly. A pass that fails
/// leaves the last published instant in place.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the floor/ceiling or the name is
/// rejected, or when the supervisor refuses the registration.
pub(super) fn register_notification_schedule<F, Fut>(
    supervisor: &TaskSupervisor,
    task_name: &'static str,
    sweep: PeriodicSweep,
    wake: DrainWakeReceiver,
    pacing: PeriodicPacing,
    operation: F,
) -> Result<(), TenantAssemblyError>
where
    F: Fn(DateTime<Utc>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Option<DateTime<Utc>>, NotificationError>> + Send + 'static,
{
    let PeriodicPacing { ceiling, pacer, due } = pacing;
    // The floor starts the backoff of a sweep that knows no due instant; a
    // reported grid point is slept to directly, below the floor included.
    let floor = Duration::from_secs(1);
    let fallback = DrainFallbackPolicy::new(floor, ceiling.max(floor)).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from(task_name).map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, fallback.schedule()),
        ShutdownPolicy::new(floor),
    );
    let operation = Arc::new(operation);
    let task = DrainTask::new(registration, fallback, move |execution| {
        let operation = Arc::clone(&operation);
        let pacer = Arc::clone(&pacer);
        let due = Arc::clone(&due);
        async move {
            pacer
                .run_drain_pass(execution.cause(), async move {
                    let cutoff = DateTime::<Utc>::from(execution.cutoff());
                    let next_due = operation(cutoff).await.map_err(classify_notification_failure)?.map(SystemTime::from);
                    due.publish(sweep, next_due);
                    match next_due {
                        Some(next_due) => Ok(DrainPassOutcome::IdleUntil(next_due)),
                        None => Ok(DrainPassOutcome::Idle),
                    }
                })
                .await
        }
    });
    supervisor.register_drain(task, wake).map_err(|error| AssemblyInvariant::from(error).into())
}

/// Register a resident task that pulses `wake` whenever a lifecycle change
/// arrives on `changes`, so the periodic sweep it drives notices a new or
/// amended subscription within its fallback floor rather than its ceiling.
///
/// A `timeInterval` subscription is only ever discovered when the sweep runs,
/// and a wake-triggered drain that nothing wakes runs only on the geometric
/// backoff bounded by `interval_max_sleep` (clause 5.8.6 fixes the period,
/// not the discovery latency). Without this bridge a subscription created just
/// after an idle pass waits out the whole ceiling before its first notification.
/// Forwarding every change to the sweep's wake is what makes the ceiling the
/// fallback the operator-facing setting documents, not the common case.
///
/// The wake coalesces, so a burst of changes costs the sweep one prompt pass; a
/// lagged stream still means the subscription set changed, so it wakes too.
///
/// The sweep is also woken once as the task starts, so a tenant loaded with a
/// grid point already owed is notified at once, not after the drain's floor. A
/// tenant woken for its periodic work is protected from unloading only for
/// `tenancy.residency.woken_lifetime`, which can be shorter than that floor.
/// The first pass also publishes the tenant's next grid point, which decides
/// how late the tenant is unloaded to make room.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the task name is rejected or the
/// supervisor refuses the registration.
pub(super) fn register_periodic_wake<T>(
    supervisor: &TaskSupervisor,
    task_name: &'static str,
    wake: WakeHandle,
    channel: ChangeChannel,
    changes: broadcast::Sender<T>,
) -> Result<(), TenantAssemblyError>
where
    T: Clone + Send + 'static,
{
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from(task_name).map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(restart_delay)),
        ShutdownPolicy::new(restart_delay),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let wake = wake.clone();
        let mut changes = changes.subscribe();
        async move {
            execution.mark_running();
            let _ = wake.wake();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = changes.recv() => match received {
                        Ok(_) | Err(RecvError::Lagged(_)) => {
                            let _ = wake.wake();
                        }
                        Err(RecvError::Closed) => return Err(channel_closed(channel)),
                    },
                }
            }
        }
    });
    supervisor
        .register_resident(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

/// Whether a notification failure ends the worker, degrades the tenant, or is
/// worth another attempt.
pub(super) const fn classify_notification_failure(error: NotificationError) -> TaskFailure<NotificationError> {
    let class = match &error {
        NotificationError::Entity(_) | NotificationError::Subscription(_) | NotificationError::Registration(_) | NotificationError::JsonLd(_) => {
            FailureClass::Transient
        }
        // A context that did not resolve may resolve on the next attempt; an
        // entity that did not compact under one that did never will.
        NotificationError::Persistence { .. } | NotificationError::Compaction(CompactionError::JsonLd(_)) => FailureClass::Transient,
        NotificationError::Representation { .. }
        | NotificationError::Rendering(_)
        | NotificationError::Compaction(CompactionError::Encode(_) | CompactionError::Refused(_)) => FailureClass::Permanent,
        NotificationError::Invariant { .. } => FailureClass::InvariantViolation,
    };
    TaskFailure::new(class, error)
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::{
        background_pacer::{BackgroundPacer, ObligationSource},
        error::TenantAssemblyError,
        periodic_due::{PeriodicDue, PeriodicSweep},
        tasks::{
            PeriodicPacing,
            notification_schedule::{register_notification_schedule, register_periodic_wake},
            notification_worker_failure::ChangeChannel,
        },
    };
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use andromeda_task_runtime::{supervisor::TaskSupervisor, wake_trigger::wake_channel};
    use async_trait::async_trait;
    use chrono::{DateTime, TimeDelta, Utc};
    use std::{
        sync::Arc,
        time::{Duration, SystemTime},
    };
    use tokio::{
        runtime::Handle,
        sync::broadcast,
        time::{sleep, timeout},
    };

    /// A tenant with no outstanding obligation, so the pacer never holds a
    /// pass back on its account.
    struct NothingOutstanding;

    #[async_trait]
    impl ObligationSource for NothingOutstanding {
        async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError> {
            Ok(None)
        }
    }

    /// Register a context source sweep whose every pass reports `next_due`,
    /// wake it once, and wait until `due` holds `expected`.
    async fn run_one_pass(due: &Arc<PeriodicDue>, next_due: Option<DateTime<Utc>>, expected: Option<SystemTime>) -> Result<bool, TenantAssemblyError> {
        let supervisor = TaskSupervisor::process(Handle::current());
        let (wake, receiver) = wake_channel();
        let pacing = PeriodicPacing {
            ceiling: Duration::from_mins(1),
            pacer: Arc::new(BackgroundPacer::new(Duration::from_secs(30), Arc::new(NothingOutstanding))),
            due: Arc::clone(due),
        };
        register_notification_schedule(
            &supervisor,
            "periodic-due-test",
            PeriodicSweep::ContextSource,
            receiver,
            pacing,
            move |_cutoff| async move { Ok(next_due) },
        )?;
        let _ = wake.wake();
        let published = timeout(Duration::from_secs(5), async {
            while due.earliest() != expected {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        supervisor.shutdown().await;
        Ok(published)
    }

    /// A new `timeInterval` Subscription wakes the sweep, and the pass that
    /// follows makes its first notification visible to the runtime cache.
    #[tokio::test]
    async fn a_sweep_pass_publishes_the_instant_it_is_next_due() -> Result<(), TenantAssemblyError> {
        let due = Arc::new(PeriodicDue::default());
        let next_due = Utc::now() + TimeDelta::seconds(1);
        let expected = SystemTime::from(DateTime::<Utc>::from_timestamp_millis(next_due.timestamp_millis()).unwrap_or(next_due));
        assert!(
            run_one_pass(&due, Some(next_due), Some(expected)).await?,
            "the pass publishes its next due instant"
        );
        Ok(())
    }

    /// The last periodic Subscription deleted, the sweep has nothing scheduled
    /// and the tenant no longer counts as having imminent work.
    #[tokio::test]
    async fn a_sweep_pass_with_nothing_scheduled_clears_what_it_published_before() -> Result<(), TenantAssemblyError> {
        let due = Arc::new(PeriodicDue::default());
        due.publish(PeriodicSweep::ContextSource, Some(SystemTime::now()));
        assert!(run_one_pass(&due, None, None).await?, "the pass clears its published instant");
        Ok(())
    }

    /// The drain's floor is a second; the first pass comes well before it.
    #[tokio::test]
    async fn a_periodic_sweep_runs_its_first_pass_as_it_starts_without_waiting_out_the_floor() -> Result<(), TenantAssemblyError> {
        let supervisor = TaskSupervisor::process(Handle::current());
        let due = Arc::new(PeriodicDue::default());
        let next_due = SystemTime::now() + Duration::from_mins(1);
        let (wake, receiver) = wake_channel();
        let (changes, _) = broadcast::channel::<()>(1);
        register_periodic_wake(&supervisor, "periodic-first-pass-wake", wake, ChangeChannel::Subscription, changes)?;
        let pacing = PeriodicPacing {
            ceiling: Duration::from_mins(1),
            pacer: Arc::new(BackgroundPacer::new(Duration::from_secs(30), Arc::new(NothingOutstanding))),
            due: Arc::clone(&due),
        };
        register_notification_schedule(
            &supervisor,
            "periodic-first-pass",
            PeriodicSweep::ContextSource,
            receiver,
            pacing,
            move |_cutoff| async move { Ok(Some(DateTime::<Utc>::from(next_due))) },
        )?;
        let published = timeout(Duration::from_millis(500), async {
            while due.earliest().is_none() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        supervisor.shutdown().await;
        assert!(published, "the first pass runs before the floor");
        Ok(())
    }
}
