//! Starting the tenant runtime cache before the broker serves traffic.

use crate::{
    tenant_janitor::{TenantJanitorRegistrationError, register_tenant_janitor},
    tenant_loading::{TenantLoadingRegistrationError, register_tenant_loading},
    tenant_waker::{TenantWakerRegistrationError, register_tenant_waker},
};
use andromeda_configuration::schema::tenancy::TenancyConfig;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_tenancy::{
    catch_up::CatchUpCandidates,
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    error::TenancyError,
    pending_work::PendingWorkProbe,
    runtime::TenantRuntimeManager,
    state_factory::TenantStateFactory,
    tenant_ref::TenantRef,
    wake_schedule::TenantWakeSchedule,
};
use std::sync::Arc;
use thiserror::Error;

/// Why the tenant runtime cache could not be started.
#[derive(Debug, Error)]
pub(crate) enum TenancyStartupError {
    /// The task supervising tenant loading could not be registered.
    #[error(transparent)]
    LoadingTask(#[from] TenantLoadingRegistrationError),
    /// The tenant runtime janitor could not be registered.
    #[error(transparent)]
    JanitorTask(#[from] TenantJanitorRegistrationError),
    /// The dormant-tenant wake tasks could not be registered.
    #[error(transparent)]
    WakerTask(#[from] TenantWakerRegistrationError),
    /// The default tenant could not be constructed and published.
    #[error("default tenant startup failed: {0}")]
    DefaultTenant(#[from] TenancyError),
}

/// Start the tenant runtime cache over `registry` and `factory`: the slots
/// tenants are loaded in, the janitor, the wake tasks, and the unnamed
/// default's runtime, built before the first request.
///
/// # Errors
///
/// Returns [`TenancyStartupError`] when a task cannot be registered on
/// `process_tasks` or the unnamed default's runtime cannot be built.
pub(crate) async fn start_tenancy<D: TenantDatabase, F: TenantStateFactory>(
    tenancy: &TenancyConfig,
    process_tasks: &TaskSupervisor,
    registry: &Arc<D>,
    factory: &Arc<F>,
    pending_work_probe: Arc<dyn PendingWorkProbe>,
    wake_schedule: Arc<dyn TenantWakeSchedule>,
    catch_up: Arc<dyn CatchUpCandidates>,
) -> Result<Arc<TenantRuntimeManager<D, F>>, TenancyStartupError> {
    let loading_slots = register_tenant_loading(process_tasks, &tenancy.build)?;
    let tenants = TenantRuntimeManager::new(
        tenancy.clone(),
        Arc::clone(registry),
        Arc::clone(factory),
        pending_work_probe,
        Arc::clone(&wake_schedule),
        loading_slots,
    );
    register_tenant_janitor(process_tasks, Arc::clone(&tenants), tenancy)?;
    // The wake tasks share the schedule with the manager and run only when named
    // tenancy is enabled; registered after the janitor so a woken runtime is
    // subject to the same eviction sweeps.
    register_tenant_waker(process_tasks, Arc::clone(&tenants), wake_schedule, catch_up, tenancy)?;
    // Warm the unnamed default so its runtime is built before the first
    // request. The returned lease is dropped at once: the default is never
    // evicted, so it needs no hold to stay resident.
    drop(tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?);
    Ok(tenants)
}

#[cfg(test)]
mod tests {
    use crate::tenancy_startup::{TenancyStartupError, start_tenancy};
    use andromeda_configuration::schema::tenancy::TenancyConfig;
    use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
    use andromeda_task_runtime::{supervisor::TaskSupervisor, task_name::TaskName};
    use andromeda_tenancy::{
        catch_up::{CatchUpCandidates, CatchUpError},
        creation_policy::TenantCreationPolicy,
        error::TenancyError,
        resident_tenancy::{FixedRegistry, FixedStateFactory, InertRuntime, NoWakes, NothingDue},
        runtime::TenantRuntimeManager,
        runtime_build_failure::RuntimeBuildFailure,
        state_factory::TenantStateFactory,
        tenant_ref::TenantRef,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::runtime::Handle;

    /// No tenant is behind the server's extension versions.
    struct NoneLagging;

    #[async_trait]
    impl CatchUpCandidates for NoneLagging {
        async fn lagging_tenants(&self) -> Result<Vec<TenantName>, CatchUpError> {
            Ok(Vec::new())
        }
    }

    /// A factory that cannot build any runtime.
    struct RefusingFactory;

    #[async_trait]
    impl TenantStateFactory for RefusingFactory {
        type State = String;
        type Runtime = InertRuntime;

        async fn build(&self, tenant: &TenantRef, _database: &TenantDatabaseName) -> Result<(Arc<String>, InertRuntime), TenancyError> {
            Err(TenancyError::RuntimeUnavailable {
                tenant: tenant.clone(),
                cause: RuntimeBuildFailure::Invariant,
            })
        }
    }

    /// Tenancy started on `supervisor` over no named tenant, building through `factory`.
    async fn started<F: TenantStateFactory>(
        supervisor: &TaskSupervisor,
        factory: F,
    ) -> Result<Arc<TenantRuntimeManager<FixedRegistry, F>>, TenancyStartupError> {
        start_tenancy(
            &TenancyConfig::default(),
            supervisor,
            &Arc::new(FixedRegistry::new(Vec::new())),
            &Arc::new(factory),
            Arc::new(NothingDue),
            Arc::new(NoWakes),
            Arc::new(NoneLagging),
        )
        .await
    }

    #[tokio::test]
    async fn starting_tenancy_supervises_the_loading_task_and_the_janitor_and_serves_the_default_tenant() {
        let supervisor = TaskSupervisor::process(Handle::current());

        let tenants = started(&supervisor, FixedStateFactory::new(Arc::new("default".to_owned())))
            .await
            .expect("tenancy starts");

        for task in ["tenant-loading", "tenant-janitor"] {
            let name = TaskName::try_from(task).expect("a valid task name");
            let health = supervisor.health(&name).await.expect("the task is registered");
            assert!(!health.state.is_terminal(), "{task}: {:?}", health.state);
        }
        let served = tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await;
        pretty_assertions::assert_eq!(served.ok().map(|leased| leased.state.as_str().to_owned()), Some("default".to_owned()));
    }

    #[tokio::test]
    async fn a_second_start_on_one_supervisor_is_refused_for_its_loading_task() {
        let supervisor = TaskSupervisor::process(Handle::current());
        assert!(started(&supervisor, FixedStateFactory::new(Arc::new("default".to_owned()))).await.is_ok());

        let again = started(&supervisor, FixedStateFactory::new(Arc::new("default".to_owned()))).await;

        assert!(matches!(again, Err(TenancyStartupError::LoadingTask(_))), "{:?}", again.err());
    }

    #[tokio::test]
    async fn a_default_tenant_that_cannot_be_built_fails_the_start() {
        let supervisor = TaskSupervisor::process(Handle::current());

        let refused = started(&supervisor, RefusingFactory).await;

        assert!(
            matches!(
                refused,
                Err(TenancyStartupError::DefaultTenant(TenancyError::RuntimeUnavailable {
                    cause: RuntimeBuildFailure::Invariant,
                    ..
                }))
            ),
            "{:?}",
            refused.err()
        );
    }
}
