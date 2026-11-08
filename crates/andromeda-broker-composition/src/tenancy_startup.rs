//! Starting the tenant runtime cache before the broker serves traffic.

use crate::{
    tenant_factory::BrokerTenantFactory,
    tenant_provisioning::{TenantProvisioningRegistrationError, register_tenant_provisioning},
    tenant_waker::{TenantWakerRegistrationError, register_tenant_waker},
};
use andromeda_configuration::broker_config::BrokerConfig;
use andromeda_postgresql_context_store::{tenant_extension_versions::PostgresTenantExtensionVersions, tenant_registry::PostgresTenantRegistry};
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_tenancy::{
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    pending_work::PendingWorkProbe,
    runtime::TenantRuntimeManager,
    tenant_ref::TenantRef,
    wake_schedule::TenantWakeSchedule,
};
use std::sync::Arc;
use thiserror::Error;

/// Why the tenant runtime cache could not be started.
#[derive(Debug, Error)]
pub(crate) enum TenancyStartupError {
    /// The task supervising new tenants' provisioning could not be registered.
    #[error(transparent)]
    ProvisioningTask(#[from] TenantProvisioningRegistrationError),
    /// The dormant-tenant wake tasks could not be registered.
    #[error(transparent)]
    WakerTask(#[from] TenantWakerRegistrationError),
    /// The default tenant could not be constructed and published.
    #[error("default tenant startup failed: {0}")]
    DefaultTenant(#[from] TenancyError),
}

/// Start the tenant runtime cache: the slots new tenants are provisioned in,
/// the janitor, the wake tasks, and the unnamed default's runtime, built before
/// the first request.
pub(crate) async fn start_tenancy(
    config: &BrokerConfig,
    process_tasks: &TaskSupervisor,
    registry: &Arc<PostgresTenantRegistry>,
    factory: &Arc<BrokerTenantFactory>,
    pending_work_probe: Arc<dyn PendingWorkProbe>,
    wake_schedule: Arc<dyn TenantWakeSchedule>,
    extension_versions: Arc<PostgresTenantExtensionVersions>,
) -> Result<Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>, TenancyStartupError> {
    let provisioning_slots = register_tenant_provisioning(process_tasks, &config.tenancy.build)?;
    let tenants = TenantRuntimeManager::new(
        config.tenancy.clone(),
        Arc::clone(registry),
        Arc::clone(factory),
        pending_work_probe,
        Arc::clone(&wake_schedule),
        provisioning_slots,
    );
    drop(tenants.spawn_janitor());
    // The wake tasks share the schedule with the manager and run only when named
    // tenancy is enabled; registered after the janitor so a woken runtime is
    // subject to the same eviction sweeps.
    register_tenant_waker(process_tasks, Arc::clone(&tenants), wake_schedule, extension_versions, &config.tenancy)?;
    // Warm the unnamed default so its runtime is built before the first
    // request. The returned lease is dropped at once: the default is never
    // evicted, so it needs no hold to stay resident.
    drop(tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?);
    Ok(tenants)
}
