//! The supervised slots new tenants are provisioned in.
//!
//! A tenant's provisioning runs detached from the request that asked for it,
//! so it is supervised in the process scope rather than in any request. The
//! process task registered here stops every provisioning still under way when
//! the broker shuts down and waits for each to end. A provisioning cut off
//! leaves a provisional tenant, which its next provisioning resumes.

use andromeda_configuration::schema::tenancy::TenantBuildConfig;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    keyed_slots::KeyedSlots,
    registration::{TaskRegistration, TaskRegistrationError},
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::{TaskName, TaskNameError},
};
use andromeda_tenancy::runtime::provisioning_flight::ProvisioningSlot;
use std::time::Duration;
use thiserror::Error;

/// How long a stopping provisioning has to reach its next await point, and
/// the pause before the stopping task is restarted after a failure.
const PROVISIONING_GRACE: Duration = Duration::from_secs(1);

/// Why the tenant provisioning task could not be registered.
#[derive(Debug, Error)]
pub(crate) enum TenantProvisioningRegistrationError {
    /// The fixed task name is invalid.
    #[error("invalid tenant provisioning task name: {0}")]
    Name(#[from] TaskNameError),
    /// A process task already owns the fixed name.
    #[error("tenant provisioning task registration failed: {0}")]
    Registration(#[from] TaskRegistrationError),
}

/// Create the slots every new tenant is provisioned in, as many as tenants
/// may load at once, and register the process task that stops them and waits
/// for each provisioning to end when the broker shuts down.
///
/// # Errors
///
/// Returns [`TenantProvisioningRegistrationError`] when the task name is
/// rejected or the supervisor refuses the registration.
pub(crate) fn register_tenant_provisioning(
    supervisor: &TaskSupervisor,
    build: &TenantBuildConfig,
) -> Result<KeyedSlots<ProvisioningSlot>, TenantProvisioningRegistrationError> {
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-provisioning")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(PROVISIONING_GRACE)),
        ShutdownPolicy::new(PROVISIONING_GRACE),
    );
    supervisor
        .register_keyed_slots(registration, build.max_concurrent)
        .map_err(TenantProvisioningRegistrationError::from)
}

#[cfg(test)]
mod tests {
    use crate::tenant_provisioning::{TenantProvisioningRegistrationError, register_tenant_provisioning};
    use andromeda_configuration::schema::tenancy::TenantBuildConfig;
    use andromeda_task_runtime::{supervisor::TaskSupervisor, task_name::TaskName};
    use tokio::runtime::Handle;

    #[tokio::test]
    async fn the_provisioning_slots_are_supervised_as_one_process_task() {
        let supervisor = TaskSupervisor::process(Handle::current());

        let registered = register_tenant_provisioning(&supervisor, &TenantBuildConfig::default());

        assert!(registered.is_ok(), "{:?}", registered.err());
        let name = TaskName::try_from("tenant-provisioning").expect("a valid task name");
        let health = supervisor.health(&name).await.expect("the provisioning task is registered");
        assert!(!health.state.is_terminal(), "{:?}", health.state);
    }

    #[tokio::test]
    async fn the_provisioning_slots_are_registered_once_per_process() {
        let supervisor = TaskSupervisor::process(Handle::current());
        assert!(register_tenant_provisioning(&supervisor, &TenantBuildConfig::default()).is_ok());

        let again = register_tenant_provisioning(&supervisor, &TenantBuildConfig::default());

        assert!(matches!(again, Err(TenantProvisioningRegistrationError::Registration(_))), "{:?}", again.err());
    }
}
