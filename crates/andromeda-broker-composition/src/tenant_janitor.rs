//! Process-scoped registration of the tenant runtime janitor.
//!
//! The janitor sweeps the runtime cache for orphaned, excess and idle runtimes,
//! settles provisional tenants, and releases a suspended tenant's retired
//! runtime once its last lease drops. It runs as a supervised resident task: an
//! attempt that ends is restarted and every failure is recorded in the task's
//! health, and broker shutdown stops it between sweeps, so a runtime a sweep
//! took out of the cache always reaches its teardown. A single-tenant broker
//! holds one runtime that is never evicted, so it registers none.

use andromeda_configuration::schema::tenancy::TenancyConfig;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::TaskFailure,
    registration::{TaskRegistration, TaskRegistrationError},
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::{TaskName, TaskNameError},
};
use andromeda_tenancy::{database::TenantDatabase, runtime::TenantRuntimeManager, state_factory::TenantStateFactory};
use std::{convert::Infallible, sync::Arc, time::Duration};
use thiserror::Error;

/// How long the janitor waits before it starts again after an attempt ended.
const JANITOR_RESTART_DELAY: Duration = Duration::from_secs(1);

/// How long a sweep under way when the broker stops may take to finish.
const JANITOR_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// Failure while registering the tenant runtime janitor.
#[derive(Debug, Error)]
pub(crate) enum TenantJanitorRegistrationError {
    /// The fixed task name is invalid.
    #[error("invalid tenant janitor task name: {0}")]
    Name(#[from] TaskNameError),
    /// A process task already owns the janitor's name.
    #[error("tenant janitor registration failed: {0}")]
    Registration(#[from] TaskRegistrationError),
}

/// Register the janitor of `manager`'s runtime cache, unless named tenancy is
/// off.
pub(crate) fn register_tenant_janitor<D: TenantDatabase, F: TenantStateFactory>(
    supervisor: &TaskSupervisor,
    manager: Arc<TenantRuntimeManager<D, F>>,
    tenancy: &TenancyConfig,
) -> Result<(), TenantJanitorRegistrationError> {
    if !tenancy.is_enabled() {
        return Ok(());
    }
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-janitor")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(JANITOR_RESTART_DELAY)),
        ShutdownPolicy::new(JANITOR_SHUTDOWN_GRACE),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let manager = Arc::clone(&manager);
        async move {
            execution.mark_running();
            manager.run_janitor(execution.cancellation().cancelled()).await;
            Ok::<(), TaskFailure<Infallible>>(())
        }
    });
    supervisor.register_resident(task).map_err(TenantJanitorRegistrationError::from)
}

#[cfg(test)]
mod tests {
    use crate::{
        tenant_janitor::{TenantJanitorRegistrationError, register_tenant_janitor},
        tenant_loading::register_tenant_loading,
    };
    use andromeda_configuration::schema::{feature_mode::FeatureMode, tenancy::TenancyConfig};
    use andromeda_task_runtime::{
        supervisor::TaskSupervisor,
        task_health::{TaskHealthSnapshot, TaskHealthState},
        task_name::TaskName,
    };
    use andromeda_tenancy::{
        resident_tenancy::{FixedRegistry, FixedStateFactory, NoWakes, NothingDue},
        runtime::TenantRuntimeManager,
    };
    use std::{sync::Arc, time::Duration};
    use tokio::{
        runtime::Handle,
        time::{sleep, timeout},
    };

    /// The janitor's health on `supervisor`, once its state is no longer
    /// `Registered`, or `None` when no janitor is registered.
    async fn settled_health(supervisor: &TaskSupervisor) -> Option<TaskHealthSnapshot> {
        let name = TaskName::try_from("tenant-janitor").expect("a valid task name");
        timeout(Duration::from_secs(5), async {
            loop {
                match supervisor.health(&name).await {
                    Some(health) if health.state == TaskHealthState::Registered => sleep(Duration::from_millis(5)).await,
                    settled => return settled,
                }
            }
        })
        .await
        .expect("the janitor leaves its registered state")
    }

    /// Register the janitor of a manager over no named tenant on `supervisor`.
    fn registered(supervisor: &TaskSupervisor, tenancy: &TenancyConfig) -> Result<(), TenantJanitorRegistrationError> {
        let loading_slots = register_tenant_loading(supervisor, &tenancy.build).expect("the loading task registers");
        let manager = TenantRuntimeManager::new(
            tenancy.clone(),
            Arc::new(FixedRegistry::new(Vec::new())),
            Arc::new(FixedStateFactory::new(Arc::new("default".to_owned()))),
            Arc::new(NothingDue),
            Arc::new(NoWakes),
            loading_slots,
        );
        register_tenant_janitor(supervisor, manager, tenancy)
    }

    #[tokio::test]
    async fn a_multi_tenant_broker_runs_its_janitor_under_supervision() {
        let supervisor = TaskSupervisor::process(Handle::current());

        registered(&supervisor, &TenancyConfig::default()).expect("the janitor registers");

        let health = settled_health(&supervisor).await.expect("the janitor is registered");
        pretty_assertions::assert_eq!(health.state, TaskHealthState::Running);
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn the_janitor_stops_when_the_broker_shuts_down_rather_than_failing() {
        let supervisor = TaskSupervisor::process(Handle::current());
        registered(&supervisor, &TenancyConfig::default()).expect("the janitor registers");
        settled_health(&supervisor).await.expect("the janitor is registered");

        supervisor.shutdown().await;

        let health = settled_health(&supervisor).await.expect("the janitor stays registered");
        pretty_assertions::assert_eq!(health.state, TaskHealthState::Cancelled);
        pretty_assertions::assert_eq!(health.consecutive_failures, 0);
    }

    #[tokio::test]
    async fn a_single_tenant_broker_registers_no_janitor() {
        let supervisor = TaskSupervisor::process(Handle::current());
        let tenancy = TenancyConfig {
            mode: FeatureMode::Disabled,
            ..TenancyConfig::default()
        };

        registered(&supervisor, &tenancy).expect("nothing is refused");

        assert!(settled_health(&supervisor).await.is_none());
    }
}
