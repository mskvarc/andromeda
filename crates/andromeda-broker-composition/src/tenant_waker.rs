//! Process-scoped dormant-tenant wake task registration.
//!
//! Supervised tasks drive the wake schedule this broker shares with its
//! replicas. The waker claims tenants whose recorded wake instant has passed
//! and loads each to run its due background work, then spends what is left of
//! its pass on tenants behind the server's extension versions. A pass that
//! claims its whole budget is followed at once by the next; any other pass
//! sleeps until the earliest wake the schedule holds, so a tenant is woken at
//! its own instant rather than on a tick. A pass that finds no room for a
//! woken tenant claims nothing and sleeps until a place frees. The waker's
//! alarm ends a sleep early when this process records a wake due sooner, or
//! when a place frees for a waker sleeping for room. The reconciliation sweep
//! re-probes the least-recently-probed tenants, so a wake a crashed replica
//! recorded but never fired is eventually picked up. All of them run only when
//! named tenancy is enabled: a single-tenant broker holds one never-evicted
//! runtime with no dormancy to manage.

use andromeda_configuration::schema::tenancy::TenancyConfig;
use andromeda_retry::{schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::{FailureClass, TaskFailure},
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError},
    registration::{TaskRegistration, TaskRegistrationError},
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::{TaskName, TaskNameError},
    wake_trigger::{DrainFallbackPolicy, WakeHandle, wake_channel},
};
use andromeda_tenancy::{
    catch_up::{CatchUpCandidates, CatchUpError},
    database::TenantDatabase,
    runtime::{
        TenantRuntimeManager,
        wake_pass::{NextWakePass, WakePassError},
    },
    state_factory::TenantStateFactory,
    wake_schedule::{TenantWakeSchedule, WakeScheduleError},
};
use std::{convert::Infallible, sync::Arc, time::Duration};
use thiserror::Error;
use tracing::debug;

/// The first sleep of a waker whose schedule holds no wake, doubling from
/// there to [`WAKER_CEILING`] while it stays empty.
const WAKER_FLOOR: Duration = Duration::from_secs(1);
/// The longest the waker sleeps between passes.
///
/// Wakes this process records ring the waker's alarm, so the ceiling bounds
/// only how late the waker sees a wake another broker process recorded, and a
/// tenant that fell behind the server's extension versions.
const WAKER_CEILING: Duration = Duration::from_secs(5);
/// The restart delay of the task that rings the waker.
const ALARM_RESTART_DELAY: Duration = Duration::from_secs(1);
/// How often the reconciliation sweep re-probes the least-recently-probed
/// tenants, and the jitter that spreads it across replicas.
///
/// The eviction path schedules a tenant's wake directly (probing after the
/// runtime's shutdown has settled its database), so reconciliation is a
/// backstop: it re-probes a tenant a crashed replica never settled, or one whose
/// eviction-time probe read a transient state. Kept brisk so a straggler is
/// picked up in tens of seconds rather than minutes, and jittered so replicas do
/// not sweep in lockstep.
const RECONCILIATION_CADENCE: Duration = Duration::from_secs(15);
/// The jitter applied to the reconciliation cadence so replicas do not sweep in
/// lockstep.
const RECONCILIATION_JITTER: Duration = Duration::from_secs(5);
/// How long a claimed wake is leased before it re-fires, covering a replica that
/// crashes between claiming a tenant and settling its schedule.
const CLAIM_LEASE: Duration = Duration::from_secs(60);

/// Failure while assembling the dormant-tenant wake schedule.
#[derive(Debug, Error)]
pub(crate) enum TenantWakerRegistrationError {
    /// The fixed cadence or jitter is invalid.
    #[error("invalid tenant wake schedule: {0}")]
    Schedule(#[from] PeriodicScheduleError),
    /// The waker's fixed floor or ceiling is invalid.
    #[error("invalid tenant waker sleep: {0}")]
    Sleep(#[from] BackoffScheduleError),
    /// A fixed task name is invalid.
    #[error("invalid tenant wake task name: {0}")]
    Name(#[from] TaskNameError),
    /// A process task already owns a fixed name.
    #[error("tenant wake task registration failed: {0}")]
    Registration(#[from] TaskRegistrationError),
}

/// Register the waker and reconciliation tasks, unless named tenancy is off.
pub(crate) fn register_tenant_waker<D: TenantDatabase, F: TenantStateFactory>(
    supervisor: &TaskSupervisor,
    manager: Arc<TenantRuntimeManager<D, F>>,
    schedule: Arc<dyn TenantWakeSchedule>,
    catch_up: Arc<dyn CatchUpCandidates>,
    tenancy: &TenancyConfig,
) -> Result<(), TenantWakerRegistrationError> {
    if !tenancy.is_enabled() {
        return Ok(());
    }
    register_waker(supervisor, Arc::clone(&manager), catch_up)?;
    register_reconciliation(supervisor, manager, schedule, tenancy.residency.max_wakes_per_pass.get())
}

/// Register the task that wakes the due tenants, then catches up lagging ones,
/// and the task that rings it when its alarm does.
fn register_waker<D: TenantDatabase, F: TenantStateFactory>(
    supervisor: &TaskSupervisor,
    manager: Arc<TenantRuntimeManager<D, F>>,
    catch_up: Arc<dyn CatchUpCandidates>,
) -> Result<(), TenantWakerRegistrationError> {
    let fallback = DrainFallbackPolicy::new(WAKER_FLOOR, WAKER_CEILING)?;
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-waker")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, fallback.schedule()),
        ShutdownPolicy::new(Duration::from_secs(10)),
    );
    let (wake, wakes) = wake_channel();
    register_alarm(supervisor, Arc::clone(&manager), wake)?;
    let task = DrainTask::new(registration, fallback, move |execution| {
        let manager = Arc::clone(&manager);
        let catch_up = Arc::clone(&catch_up);
        async move {
            let now = execution.cutoff();
            let report = manager
                .run_wake_pass(catch_up.as_ref(), now, now + CLAIM_LEASE)
                .await
                .map_err(classify_pass_failure)?;
            if report.scheduled > 0 || report.caught_up > 0 {
                debug!(
                    scheduled = report.scheduled,
                    caught_up = report.caught_up,
                    "tenant waker loaded dormant tenants"
                );
            }
            Ok(next_pass(report.next))
        }
    });
    supervisor.register_drain(task, wakes).map_err(TenantWakerRegistrationError::from)
}

/// When the waker's drain runs its next pass.
const fn next_pass(next: NextWakePass) -> DrainPassOutcome {
    match next {
        NextWakePass::Immediately => DrainPassOutcome::WorkMayRemain,
        NextWakePass::At(at) => DrainPassOutcome::IdleUntil(at),
        NextWakePass::WhenRecorded | NextWakePass::WhenRoomOpens => DrainPassOutcome::Idle,
    }
}

/// Register the task that wakes the waker each time its alarm rings.
fn register_alarm<D: TenantDatabase, F: TenantStateFactory>(
    supervisor: &TaskSupervisor,
    manager: Arc<TenantRuntimeManager<D, F>>,
    wake: WakeHandle,
) -> Result<(), TenantWakerRegistrationError> {
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-wake-alarm")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(ALARM_RESTART_DELAY)),
        ShutdownPolicy::new(ALARM_RESTART_DELAY),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let manager = Arc::clone(&manager);
        let wake = wake.clone();
        async move {
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok::<(), TaskFailure<Infallible>>(()),
                    () = manager.wake_alarm().rung() => {
                        let _ = wake.wake();
                    }
                }
            }
        }
    });
    supervisor.register_resident(task).map_err(TenantWakerRegistrationError::from)
}

/// Register the crash-recovery sweep that re-probes the least-recently-probed
/// tenants so a wake a crashed replica never fired is eventually picked up.
fn register_reconciliation<D: TenantDatabase, F: TenantStateFactory>(
    supervisor: &TaskSupervisor,
    manager: Arc<TenantRuntimeManager<D, F>>,
    schedule: Arc<dyn TenantWakeSchedule>,
    limit: usize,
) -> Result<(), TenantWakerRegistrationError> {
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-wake-reconciliation")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(RECONCILIATION_CADENCE)),
        ShutdownPolicy::new(Duration::from_secs(10)),
    );
    let schedule_pass =
        PeriodicSchedule::new(RECONCILIATION_CADENCE, InitialExecution::Immediate, MissedTickPolicy::Skip)?.with_jitter(RECONCILIATION_JITTER)?;
    let task = ScheduledTask::new(registration, schedule_pass, move |_execution| {
        let manager = Arc::clone(&manager);
        let schedule = Arc::clone(&schedule);
        async move {
            let unprobed = schedule.next_unprobed(limit).await.map_err(classify_failure)?;
            for tenant in unprobed {
                manager.reconcile(&tenant).await;
            }
            Ok(())
        }
    });
    supervisor.register_scheduled(task).map(drop).map_err(TenantWakerRegistrationError::from)
}

/// A wake pass failure is transient: a leased claim re-fires, and the next pass
/// reads the catch-up candidates again.
const fn classify_pass_failure(error: WakePassError) -> TaskFailure<WakePassError> {
    let class = match &error {
        WakePassError::Schedule(WakeScheduleError::Store { .. }) | WakePassError::CatchUp(CatchUpError::Store { .. }) => FailureClass::Transient,
    };
    TaskFailure::new(class, error)
}

/// A wake-schedule store failure is transient: the leased claim re-fires and the
/// next pass retries.
const fn classify_failure(error: WakeScheduleError) -> TaskFailure<WakeScheduleError> {
    let class = match &error {
        WakeScheduleError::Store { .. } => FailureClass::Transient,
    };
    TaskFailure::new(class, error)
}

#[cfg(test)]
mod tests {
    use crate::tenant_waker::next_pass;
    use andromeda_task_runtime::drain_task::DrainPassOutcome;
    use andromeda_tenancy::runtime::wake_pass::NextWakePass;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_pass_that_claimed_its_whole_budget_is_followed_at_once() {
        pretty_assertions::assert_eq!(next_pass(NextWakePass::Immediately), DrainPassOutcome::WorkMayRemain);
    }

    #[test]
    fn a_pass_that_left_budget_over_sleeps_until_the_earliest_wake() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        pretty_assertions::assert_eq!(next_pass(NextWakePass::At(at)), DrainPassOutcome::IdleUntil(at));
    }

    #[test]
    fn a_pass_with_no_wake_scheduled_sleeps_until_one_is_recorded_or_the_ceiling() {
        pretty_assertions::assert_eq!(next_pass(NextWakePass::WhenRecorded), DrainPassOutcome::Idle);
    }

    #[test]
    fn a_pass_with_no_room_and_no_woken_lifetime_ending_sleeps_until_a_place_frees_or_the_ceiling() {
        pretty_assertions::assert_eq!(next_pass(NextWakePass::WhenRoomOpens), DrainPassOutcome::Idle);
    }
}
