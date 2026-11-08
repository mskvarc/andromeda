//! The supervised slots tenants are loaded in.
//!
//! A new tenant's provisioning and the build of a tenant's runtime run
//! detached from the request that asked for them, so they are supervised in
//! the process scope rather than in any request. The process task registered
//! here stops every provisioning and build still under way when the broker
//! shuts down and waits for each to end. A provisioning cut off leaves a
//! provisional tenant, which its next provisioning resumes.

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
use andromeda_tenancy::runtime::flight::FlightSlot;
use std::time::Duration;
use thiserror::Error;

/// How long a stopping provisioning or build has to reach its next await point, and
/// the pause before the stopping task is restarted after a failure.
const LOADING_GRACE: Duration = Duration::from_secs(1);

/// Why the tenant loading task could not be registered.
#[derive(Debug, Error)]
pub(crate) enum TenantLoadingRegistrationError {
    /// The fixed task name is invalid.
    #[error("invalid tenant loading task name: {0}")]
    Name(#[from] TaskNameError),
    /// A process task already owns the fixed name.
    #[error("tenant loading task registration failed: {0}")]
    Registration(#[from] TaskRegistrationError),
}

/// Create the slots every new tenant is provisioned and every runtime built
/// in, as many as tenants may load at once, and register the process task
/// that stops them and waits for each to end when the broker shuts down.
///
/// # Errors
///
/// Returns [`TenantLoadingRegistrationError`] when the task name is
/// rejected or the supervisor refuses the registration.
pub(crate) fn register_tenant_loading(
    supervisor: &TaskSupervisor,
    build: &TenantBuildConfig,
) -> Result<KeyedSlots<FlightSlot>, TenantLoadingRegistrationError> {
    let registration = TaskRegistration::new(
        TaskName::try_from("tenant-loading")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(LOADING_GRACE)),
        ShutdownPolicy::new(LOADING_GRACE),
    );
    supervisor
        .register_keyed_slots(registration, build.max_concurrent)
        .map_err(TenantLoadingRegistrationError::from)
}

#[cfg(test)]
mod tests {
    use crate::tenant_loading::{TenantLoadingRegistrationError, register_tenant_loading};
    use andromeda_configuration::schema::tenancy::TenantBuildConfig;
    use andromeda_task_runtime::{supervisor::TaskSupervisor, task_name::TaskName};
    use tokio::runtime::Handle;

    #[tokio::test]
    async fn the_loading_slots_are_supervised_as_one_process_task() {
        let supervisor = TaskSupervisor::process(Handle::current());

        let registered = register_tenant_loading(&supervisor, &TenantBuildConfig::default());

        assert!(registered.is_ok(), "{:?}", registered.err());
        let name = TaskName::try_from("tenant-loading").expect("a valid task name");
        let health = supervisor.health(&name).await.expect("the loading task is registered");
        assert!(!health.state.is_terminal(), "{:?}", health.state);
    }

    #[tokio::test]
    async fn the_loading_slots_are_registered_once_per_process() {
        let supervisor = TaskSupervisor::process(Handle::current());
        assert!(register_tenant_loading(&supervisor, &TenantBuildConfig::default()).is_ok());

        let again = register_tenant_loading(&supervisor, &TenantBuildConfig::default());

        assert!(matches!(again, Err(TenantLoadingRegistrationError::Registration(_))), "{:?}", again.err());
    }
}
