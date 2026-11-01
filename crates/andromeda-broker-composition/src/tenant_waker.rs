//! Process-scoped dormant-tenant wake task registration.
//!
//! Two supervised tasks drive the wake schedule this broker shares with its
//! replicas. The waker claims tenants whose recorded wake instant has passed
//! and loads each to run its due background work, then spends what is left of
//! its pass on tenants behind the server's extension versions; the
//! reconciliation sweep
//! re-probes the least-recently-probed tenants, so a wake a crashed replica
//! recorded but never fired is eventually picked up. Both only run when named
//! tenancy is enabled -- a single-tenant broker holds one never-evicted runtime
//! with no dormancy to manage.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_configuration::schema::tenancy::TenancyConfig;
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::{FailureClass, TaskFailure},
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError},
    registration::{TaskRegistration, TaskRegistrationError},
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::{TaskName, TaskNameError},
};
use andromeda_tenancy::{
    catch_up::{CatchUpCandidates, CatchUpError},
    runtime::{TenantRuntimeManager, wake_pass::WakePassError},
    wake_schedule::{TenantWakeSchedule, WakeScheduleError},
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use thiserror::Error;
use tracing::debug;

/// How often the waker claims due tenants.
const WAKER_INTERVAL: Duration = Duration::from_secs(5);
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

/// The manager whose dormant tenants the waker loads.
type Manager = Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>;

/// Failure while assembling the dormant-tenant wake schedule.
#[derive(Debug, Error)]
pub(crate) enum TenantWakerRegistrationError {
    /// The fixed cadence or jitter is invalid.
    #[error("invalid tenant wake schedule: {0}")]
    Schedule(#[from] PeriodicScheduleError),
    /// A fixed task name is invalid.
    #[error("invalid tenant wake task name: {0}")]
    Name(#[from] TaskNameError),
    /// A process task already owns a fixed name.
    #[error("tenant wake task registration failed: {0}")]
    Registration(#[from] TaskRegistrationError),
}

/// Register the waker and reconciliation tasks, unless named tenancy is off.
pub(crate) fn register_tenant_waker(
    supervisor: &TaskSupervisor,
    manager: Manager,
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

/// Register the task that wakes the due tenants, then catches up lagging ones.
fn register_waker(supervisor: &TaskSupervisor, manager: Manager, catch_up: Arc<dyn CatchUpCandidates>) -> Result<(), TenantWakerRegistrationError> {
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-waker")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(WAKER_INTERVAL)),
        ShutdownPolicy::new(Duration::from_secs(10)),
    );
    let schedule_pass = PeriodicSchedule::new(WAKER_INTERVAL, InitialExecution::Immediate, MissedTickPolicy::Skip)?;
    let task = ScheduledTask::new(registration, schedule_pass, move |_execution| {
        let manager = Arc::clone(&manager); // clone: each pass owns the manager while executing
        let catch_up = Arc::clone(&catch_up); // clone: each pass owns the catch-up source
        async move {
            let now = SystemTime::now();
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
            Ok(())
        }
    });
    supervisor.register_scheduled(task).map(drop).map_err(TenantWakerRegistrationError::from)
}

/// Register the crash-recovery sweep that re-probes the least-recently-probed
/// tenants so a wake a crashed replica never fired is eventually picked up.
fn register_reconciliation(
    supervisor: &TaskSupervisor,
    manager: Manager,
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
        let manager = Arc::clone(&manager); // clone: each pass owns the manager while executing
        let schedule = Arc::clone(&schedule); // clone: each pass owns the schedule handle
        async move {
            let unprobed = schedule.next_unprobed(limit).await.map_err(classify_failure)?;
            for tenant in unprobed {
                manager.wake(&tenant).await;
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
