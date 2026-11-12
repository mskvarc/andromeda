//! Keyed slots a tenant's tasks hand detached work to, stopped with the tenant.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    keyed_slots::KeyedSlots,
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
};
use std::{hash::Hash, num::NonZeroUsize, time::Duration};

/// How long a tenant's slots get to stop their work once the tenant shuts down.
const SLOTS_GRACE: Duration = Duration::from_secs(1);

/// Create `width` keyed slots on the tenant's `supervisor`, and register under
/// `name` the task that stops them, and waits for every work item to end, when
/// the tenant shuts down.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the task name is rejected or the
/// supervisor refuses the registration.
pub(super) fn register_tenant_slots<K: Eq + Hash + Clone + Send + Sync + 'static>(
    supervisor: &TaskSupervisor,
    name: &str,
    width: NonZeroUsize,
) -> Result<KeyedSlots<K>, TenantAssemblyError> {
    let name = TaskName::try_from(name).map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(SLOTS_GRACE)),
        ShutdownPolicy::new(SLOTS_GRACE),
    );
    supervisor
        .register_keyed_slots(registration, width)
        .map_err(|error| TenantAssemblyError::from(AssemblyInvariant::from(error)))
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::tenant_slots::register_tenant_slots;
    use andromeda_task_runtime::{keyed_slots::KeyedSlots, supervisor::TaskSupervisor, task_name::TaskName};
    use std::num::NonZeroUsize;
    use tokio::runtime::Handle;

    #[tokio::test]
    async fn registered_slots_appear_under_their_name_in_the_tenant_task_health() {
        let process = TaskSupervisor::process(Handle::current());
        let supervisor = process.tenant(0);

        let _slots: KeyedSlots<u32> = register_tenant_slots(&supervisor, "tenant-slots", NonZeroUsize::MIN).expect("the slots register");

        let name = TaskName::try_from("tenant-slots").expect("a valid task name");
        assert!(supervisor.health(&name).await.is_some(), "the slots' task is listed");
    }

    #[tokio::test]
    async fn a_second_slots_registration_under_the_same_name_is_refused() {
        let process = TaskSupervisor::process(Handle::current());
        let supervisor = process.tenant(0);
        let _first: KeyedSlots<u32> = register_tenant_slots(&supervisor, "tenant-slots", NonZeroUsize::MIN).expect("the slots register");

        let second: Result<KeyedSlots<u32>, _> = register_tenant_slots(&supervisor, "tenant-slots", NonZeroUsize::MIN);

        assert!(second.is_err(), "the name is already taken");
    }
}
