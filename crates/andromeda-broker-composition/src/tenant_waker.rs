//! Process-scoped dormant-tenant wake task registration.
//!
//! Two supervised tasks drive the wake schedule this broker shares with its
//! replicas. The waker claims tenants whose recorded wake instant has passed
//! and loads each to run its due background work; the reconciliation sweep
//! re-probes the least-recently-probed tenants, so a wake a crashed replica
//! recorded but never fired is eventually picked up. Both only run when named
//! tenancy is enabled -- a single-tenant broker holds one never-evicted runtime
//! with no dormancy to manage.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::TaskFailure,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError},
    registration::{TaskRegistration, TaskRegistrationError},
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::{TaskName, TaskNameError},
};
use andromeda_tenancy::{
    config::TenancyConfig,
    runtime::TenantRuntimeManager,
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
const RECONCILIATION_CADENCE: Duration = Duration::from_secs(60);
/// The jitter applied to the reconciliation cadence so replicas do not sweep in
/// lockstep.
const RECONCILIATION_JITTER: Duration = Duration::from_secs(15);
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
    tenancy: &TenancyConfig,
) -> Result<(), TenantWakerRegistrationError> {
    if !tenancy.enabled {
        return Ok(());
    }
    let limit = tenancy.waker_max_wakes_per_pass;
    register_waker(supervisor, Arc::clone(&manager), Arc::clone(&schedule), limit)?;
    register_reconciliation(supervisor, manager, schedule, limit)
}

/// Register the task that claims due tenants and wakes each.
fn register_waker(
    supervisor: &TaskSupervisor,
    manager: Manager,
    schedule: Arc<dyn TenantWakeSchedule>,
    limit: usize,
) -> Result<(), TenantWakerRegistrationError> {
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-waker")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, WAKER_INTERVAL, WAKER_INTERVAL),
        ShutdownPolicy::new(Duration::from_secs(10)),
    );
    let schedule_pass = PeriodicSchedule::new(WAKER_INTERVAL, InitialExecution::Immediate, MissedTickPolicy::Skip)?;
    let task = ScheduledTask::new(registration, schedule_pass, move |_execution| {
        let manager = Arc::clone(&manager); // clone: each pass owns the manager while executing
        let schedule = Arc::clone(&schedule); // clone: each pass owns the schedule handle
        async move {
            let now = SystemTime::now();
            let due = schedule.claim_due(now, now + CLAIM_LEASE, limit).await.map_err(classify_failure)?;
            let woken = due.len();
            for tenant in due {
                manager.wake(&tenant).await;
            }
            if woken > 0 {
                debug!(woken, "tenant waker loaded dormant tenants with due work");
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
        RestartPolicy::transient(u32::MAX, RECONCILIATION_CADENCE, RECONCILIATION_CADENCE),
        ShutdownPolicy::new(Duration::from_secs(10)),
    );
    let schedule_pass =
        PeriodicSchedule::new(RECONCILIATION_CADENCE, InitialExecution::AfterCadence, MissedTickPolicy::Skip)?.with_jitter(RECONCILIATION_JITTER)?;
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

/// A wake-schedule store failure is transient: the leased claim re-fires and the
/// next pass retries.
fn classify_failure(error: WakeScheduleError) -> TaskFailure {
    match error {
        WakeScheduleError::Store { detail } => TaskFailure::transient(detail),
    }
}
