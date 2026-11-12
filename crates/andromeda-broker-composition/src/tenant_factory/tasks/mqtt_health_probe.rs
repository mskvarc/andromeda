//! The sweep that marks the MQTT connections gone quiet as degraded.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::TaskFailure,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use std::{convert::Infallible, sync::Arc};

/// Register the task that marks the MQTT pool's quiet connections degraded
/// every `health_probe_interval`, starting at once, in the unnamed default
/// tenant, which exists once per process; every other tenant registers none.
///
/// The sweep only reads timestamps and flips health flags, so the task is
/// merely observed: its failure never degrades the tenant.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the pool's probe interval is rejected
/// as a cadence or when the supervisor refuses the registration.
pub(super) fn register_mqtt_health_probe(supervisor: &TaskSupervisor, tenant: &TenantRef, transport: &TransportDispatcher) -> Result<(), TenantAssemblyError> {
    if !tenant.is_unnamed_default() {
        return Ok(());
    }
    let pool = Arc::clone(transport.mqtt_pool());
    let cadence = pool.health_probe_interval();
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("mqtt-health-probe").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(cadence)),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let pool = Arc::clone(&pool);
        async move {
            pool.mark_quiet_connections_degraded().await;
            Ok::<(), TaskFailure<Infallible>>(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::mqtt_health_probe::register_mqtt_health_probe;
    use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_task_runtime::{supervisor::TaskSupervisor, task_name::TaskName};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use tokio::runtime::Handle;

    fn transport() -> TransportDispatcher {
        TransportDispatcher::new(HttpTransportConfig::default(), MqttPoolConfig::default()).expect("the default transports build")
    }

    fn probe() -> TaskName {
        TaskName::try_from("mqtt-health-probe").expect("a valid task name")
    }

    #[tokio::test]
    async fn the_unnamed_default_tenant_lists_the_probe_in_its_task_health() {
        let process = TaskSupervisor::process(Handle::current());
        let supervisor = process.tenant(0);

        register_mqtt_health_probe(&supervisor, &TenantRef::UnnamedDefault, &transport()).expect("the probe registers");

        assert!(supervisor.health(&probe()).await.is_some(), "the probe is listed");
    }

    #[tokio::test]
    async fn a_named_tenant_registers_no_probe() {
        let process = TaskSupervisor::process(Handle::current());
        let supervisor = process.tenant(0);
        let tenant = TenantRef::Named(TenantName::try_from("acme".to_owned()).expect("a valid tenant name"));

        register_mqtt_health_probe(&supervisor, &tenant, &transport()).expect("nothing to register");

        assert!(supervisor.health(&probe()).await.is_none(), "no probe is listed");
    }
}
