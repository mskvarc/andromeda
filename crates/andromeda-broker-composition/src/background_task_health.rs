//! The health of the background tasks of the broker and of its loaded tenants.
//!
//! A task whose failures outlast its restart policy stops for good. The
//! runtime cancels the tenant or the broker for the fatal criticalities and
//! otherwise only counts the failure, so a stopped `tenant-degrading` task
//! leaves its work undone while the probes would answer up. This indicator
//! reads the stopped tasks from the supervisors: the process supervisor and the
//! supervisor of every tenant runtime the cache holds. A tenant runtime that is
//! evicted drops its supervisor, so its stopped tasks leave the report with it
//! and are running again when the tenant loads.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_health::{
    background_task_details::{
        BackgroundTaskDetails,
        BackgroundTaskName,
        StoppedTask,
        StoppedTaskCriticality,
        StoppedTaskFailureClass,
        TaskTenant,
        UnnamedDefaultTenant,
    },
    component::{ComponentDetail, ComponentName},
    details::ComponentDetails,
    indicator::HealthIndicator,
    status::HealthStatus,
};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_task_runtime::{criticality::TaskCriticality, failure::FailureClass, supervisor::TaskSupervisor, task_health::TaskHealthSnapshot};
use andromeda_tenancy::{runtime::TenantRuntimeManager, tenant_ref::TenantRef};
use async_trait::async_trait;
use std::sync::Arc;

/// The supervisors of the tenant runtimes loaded right now.
pub(crate) trait LoadedTenantSupervisors: Send + Sync + 'static {
    /// The supervisor of each loaded tenant runtime, with its tenant.
    fn loaded(&self) -> Vec<(TenantRef, TaskSupervisor)>;
}

impl LoadedTenantSupervisors for TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory> {
    fn loaded(&self) -> Vec<(TenantRef, TaskSupervisor)> {
        self.read_resident(|runtime| runtime.task_supervisor().clone()) // clone: the report reads the supervisor after the cache's lock is released.
    }
}

/// Reports the background tasks that stopped for good.
///
/// The component is degraded while any task with criticality `tenant-degrading`
/// has stopped, and up otherwise. A stopped `observed` task is listed and leaves
/// the component up. The fatal criticalities are not listed: they cancel the
/// tenant or the broker, which the rest of the report already shows.
pub(crate) struct BackgroundTaskHealthIndicator {
    /// The supervisor of the broker's own tasks.
    process: TaskSupervisor,
    /// Where the supervisors of the loaded tenants come from.
    tenants: Arc<dyn LoadedTenantSupervisors>,
}

impl BackgroundTaskHealthIndicator {
    /// Report on the process supervisor and the supervisors `tenants` holds.
    pub(crate) fn new(process: TaskSupervisor, tenants: Arc<dyn LoadedTenantSupervisors>) -> BackgroundTaskHealthIndicator {
        BackgroundTaskHealthIndicator { process, tenants }
    }
}

#[async_trait]
impl HealthIndicator for BackgroundTaskHealthIndicator {
    fn name(&self) -> ComponentName {
        ComponentName::BackgroundTasks
    }

    async fn check(&self) -> ComponentDetail {
        let mut stopped_tasks = stopped_in(&self.process, None).await;
        let mut tenants = self.tenants.loaded();
        tenants.sort_by_cached_key(|(tenant, _)| tenant.to_string());
        for (tenant, supervisor) in &tenants {
            stopped_tasks.extend(stopped_in(supervisor, Some(task_tenant(tenant))).await);
        }
        let status = if stopped_tasks.iter().any(|task| task.criticality == StoppedTaskCriticality::TenantDegrading) {
            HealthStatus::Degraded
        } else {
            HealthStatus::Up
        };
        ComponentDetail {
            status,
            details: Some(ComponentDetails::BackgroundTasks(BackgroundTaskDetails { stopped_tasks })),
        }
    }
}

/// The tenant as a stopped task reports it.
fn task_tenant(tenant: &TenantRef) -> TaskTenant {
    match tenant {
        TenantRef::UnnamedDefault => TaskTenant::UnnamedDefault(UnnamedDefaultTenant),
        TenantRef::Named(name) => TaskTenant::Named(name.clone()), // clone: the report owns the name after the supervisor listing ends.
    }
}

/// The tasks of `supervisor` that stopped for good and leave their scope serving.
async fn stopped_in(supervisor: &TaskSupervisor, tenant: Option<TaskTenant>) -> Vec<StoppedTask> {
    supervisor
        .failed_tasks()
        .await
        .into_iter()
        .filter_map(|snapshot| stopped_task(snapshot, tenant.clone())) // clone: each stopped task of the tenant carries it.
        .collect()
}

/// The report of one stopped task, or `None` for a fatal criticality.
fn stopped_task(snapshot: TaskHealthSnapshot, tenant: Option<TaskTenant>) -> Option<StoppedTask> {
    let criticality = match snapshot.criticality {
        TaskCriticality::Observed => StoppedTaskCriticality::Observed,
        TaskCriticality::TenantDegrading => StoppedTaskCriticality::TenantDegrading,
        TaskCriticality::TenantFatal | TaskCriticality::ProcessFatal => return None,
    };
    // A task stops for good only through a recorded failure, so both are set.
    let (class, summary) = snapshot.last_failure_class.zip(snapshot.last_failure_summary)?;
    Some(StoppedTask {
        name: BackgroundTaskName::new(snapshot.name.to_string()),
        tenant,
        criticality,
        failure_class: match class {
            FailureClass::Transient => StoppedTaskFailureClass::Transient,
            FailureClass::Permanent => StoppedTaskFailureClass::Permanent,
            FailureClass::InvariantViolation => StoppedTaskFailureClass::InvariantViolation,
        },
        failure_summary: summary.to_string(),
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::background_task_health::{BackgroundTaskHealthIndicator, LoadedTenantSupervisors};
    use andromeda_health::{
        background_task_details::{
            BackgroundTaskDetails,
            BackgroundTaskName,
            StoppedTask,
            StoppedTaskCriticality,
            StoppedTaskFailureClass,
            TaskTenant,
            UnnamedDefaultTenant,
        },
        component::{ComponentDetail, ComponentName},
        details::ComponentDetails,
        indicator::HealthIndicator,
        status::HealthStatus,
    };
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_task_runtime::{
        criticality::TaskCriticality,
        failure::TaskFailure,
        registration::TaskRegistration,
        resident_task::ResidentTask,
        restart_policy::RestartPolicy,
        shutdown_policy::ShutdownPolicy,
        supervisor::TaskSupervisor,
        task_name::TaskName,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use std::{sync::Arc, time::Duration};
    use thiserror::Error;
    use tokio::{runtime::Handle, task::yield_now, time::advance};

    #[derive(Debug, Error)]
    #[error("scripted failure")]
    struct Scripted;

    /// A fixed set of loaded tenants.
    struct Loaded(Vec<(TenantRef, TaskSupervisor)>);

    impl LoadedTenantSupervisors for Loaded {
        fn loaded(&self) -> Vec<(TenantRef, TaskSupervisor)> {
            self.0.clone() // clone: supervisors are shared handles.
        }
    }

    fn named(raw: &str) -> TenantRef {
        TenantRef::Named(TenantName::try_from(raw.to_owned()).expect("a valid tenant name"))
    }

    fn registration(name: &str, criticality: TaskCriticality) -> TaskRegistration {
        TaskRegistration::new(
            TaskName::try_from(name).expect("a valid task name"),
            criticality,
            RestartPolicy::Never,
            ShutdownPolicy::new(Duration::from_secs(1)),
        )
    }

    async fn settle() {
        for _ in 0..8 {
            yield_now().await;
        }
        advance(Duration::from_secs(1)).await;
        for _ in 0..8 {
            yield_now().await;
        }
    }

    /// Register a task that fails at once with a permanent failure and wait until it has stopped.
    async fn stop(supervisor: &TaskSupervisor, name: &str, criticality: TaskCriticality) {
        supervisor
            .register_resident(ResidentTask::new(registration(name, criticality), |_| async {
                Err(TaskFailure::permanent(Scripted))
            }))
            .expect("registers");
        settle().await;
    }

    /// Register a task that keeps running until its scope is cancelled.
    fn keep_running(supervisor: &TaskSupervisor, name: &str) {
        supervisor
            .register_resident(ResidentTask::new(
                registration(name, TaskCriticality::TenantDegrading),
                |execution| async move {
                    execution.cancellation().cancelled().await;
                    Result::<(), TaskFailure<Scripted>>::Ok(())
                },
            ))
            .expect("registers");
    }

    fn stopped(name: &str, tenant: Option<TaskTenant>, criticality: StoppedTaskCriticality) -> StoppedTask {
        StoppedTask {
            name: BackgroundTaskName::new(name.to_owned()),
            tenant,
            criticality,
            failure_class: StoppedTaskFailureClass::Permanent,
            failure_summary: "scripted failure".to_owned(),
        }
    }

    fn reported(status: HealthStatus, stopped_tasks: Vec<StoppedTask>) -> ComponentDetail {
        ComponentDetail {
            status,
            details: Some(ComponentDetails::BackgroundTasks(BackgroundTaskDetails { stopped_tasks })),
        }
    }

    async fn check(process: &TaskSupervisor, tenants: Vec<(TenantRef, TaskSupervisor)>) -> ComponentDetail {
        BackgroundTaskHealthIndicator::new(process.clone(), Arc::new(Loaded(tenants))).check().await // clone: the indicator owns a handle to the same supervisor.
    }

    #[tokio::test(start_paused = true)]
    async fn the_indicator_reports_under_the_background_tasks_component_name() {
        let process = TaskSupervisor::process(Handle::current());
        pretty_assertions::assert_eq!(
            BackgroundTaskHealthIndicator::new(process, Arc::new(Loaded(Vec::new()))).name(),
            ComponentName::BackgroundTasks
        );
    }

    #[tokio::test(start_paused = true)]
    async fn running_tasks_in_every_scope_report_up_and_list_nothing() {
        let process = TaskSupervisor::process(Handle::current());
        let default = process.tenant(1);
        keep_running(&process, "maintenance");
        keep_running(&default, "notification-delivery");
        settle().await;

        pretty_assertions::assert_eq!(
            check(&process, vec![(TenantRef::UnnamedDefault, default)]).await,
            reported(HealthStatus::Up, Vec::new())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_degrading_task_stopped_in_one_tenant_degrades_the_component_and_names_that_tenant() {
        let process = TaskSupervisor::process(Handle::current());
        let default = process.tenant(1);
        let acme = process.tenant(2);
        stop(&default, "notification-delivery", TaskCriticality::TenantDegrading).await;
        keep_running(&acme, "notification-delivery");
        settle().await;

        pretty_assertions::assert_eq!(
            check(&process, vec![(TenantRef::UnnamedDefault, default), (named("acme"), acme)]).await,
            reported(
                HealthStatus::Degraded,
                vec![stopped(
                    "notification-delivery",
                    Some(TaskTenant::UnnamedDefault(UnnamedDefaultTenant)),
                    StoppedTaskCriticality::TenantDegrading
                )]
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_degrading_task_stopped_in_the_process_scope_degrades_the_component_without_a_tenant() {
        let process = TaskSupervisor::process(Handle::current());
        stop(&process, "maintenance", TaskCriticality::TenantDegrading).await;

        pretty_assertions::assert_eq!(
            check(&process, Vec::new()).await,
            reported(
                HealthStatus::Degraded,
                vec![stopped("maintenance", None, StoppedTaskCriticality::TenantDegrading)]
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_observed_task_stopped_is_listed_and_leaves_the_component_up() {
        let process = TaskSupervisor::process(Handle::current());
        let acme = process.tenant(1);
        stop(&acme, "pool-gauges", TaskCriticality::Observed).await;

        pretty_assertions::assert_eq!(
            check(&process, vec![(named("acme"), acme)]).await,
            reported(
                HealthStatus::Up,
                vec![stopped(
                    "pool-gauges",
                    Some(TaskTenant::Named(TenantName::try_from("acme".to_owned()).expect("a valid tenant name"))),
                    StoppedTaskCriticality::Observed
                )]
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn tasks_of_two_tenants_are_listed_in_tenant_order_after_the_process_tasks() {
        let process = TaskSupervisor::process(Handle::current());
        let zeta = process.tenant(1);
        let alpha = process.tenant(2);
        stop(&process, "maintenance", TaskCriticality::Observed).await;
        stop(&zeta, "expiry", TaskCriticality::Observed).await;
        stop(&alpha, "expiry", TaskCriticality::Observed).await;

        let tenant = |raw: &str| Some(TaskTenant::Named(TenantName::try_from(raw.to_owned()).expect("a valid tenant name")));

        pretty_assertions::assert_eq!(
            check(&process, vec![(named("zeta"), zeta), (named("alpha"), alpha)]).await,
            reported(
                HealthStatus::Up,
                vec![
                    stopped("maintenance", None, StoppedTaskCriticality::Observed),
                    stopped("expiry", tenant("alpha"), StoppedTaskCriticality::Observed),
                    stopped("expiry", tenant("zeta"), StoppedTaskCriticality::Observed),
                ]
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_fatal_task_that_stopped_is_not_listed() {
        let process = TaskSupervisor::process(Handle::current());
        let acme = process.tenant(1);
        stop(&acme, "snapshot", TaskCriticality::TenantFatal).await;

        pretty_assertions::assert_eq!(check(&process, vec![(named("acme"), acme)]).await, reported(HealthStatus::Up, Vec::new()));
    }

    #[tokio::test(start_paused = true)]
    async fn an_evicted_tenant_leaves_the_report_and_the_component_returns_to_up() {
        let process = TaskSupervisor::process(Handle::current());
        let acme = process.tenant(1);
        stop(&acme, "notification-delivery", TaskCriticality::TenantDegrading).await;
        pretty_assertions::assert_eq!(check(&process, vec![(named("acme"), acme)]).await.status, HealthStatus::Degraded);

        pretty_assertions::assert_eq!(check(&process, Vec::new()).await, reported(HealthStatus::Up, Vec::new()));
    }
}
