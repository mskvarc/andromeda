//! The health the probes report.

use crate::{
    background_task_health::{BackgroundTaskHealthIndicator, LoadedTenantSupervisors},
    tenant_factory::BrokerTenantFactory,
};
use andromeda_delivery::mqtt::indicator::MqttHealthIndicator;
use andromeda_health::aggregate::Health;
use andromeda_postgresql_context_store::{health::DatabaseHealthIndicator, tenant_registry::PostgresTenantRegistry};
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_tenancy::runtime::TenantRuntimeManager;
use std::sync::Arc;

/// The health the probes report: the main database, the background tasks of
/// the broker and of its loaded tenants and, when the default tenant delivers
/// over MQTT, its MQTT pool.
pub(crate) fn broker_health(
    registry: &PostgresTenantRegistry,
    factory: &BrokerTenantFactory,
    process_tasks: &TaskSupervisor,
    tenants: &Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
) -> Health {
    let database_indicator = DatabaseHealthIndicator::new(registry.main_pool().clone()); // clone: PgPool is a shared handle
    let task_indicator = BackgroundTaskHealthIndicator::new(process_tasks.clone(), Arc::clone(tenants) as Arc<dyn LoadedTenantSupervisors>); // clone: the indicator reads the process supervisor and the manager the broker keeps
    let mut health = Health::builder().with_indicator(database_indicator).with_indicator(task_indicator);
    if let Some(mqtt_pool) = factory.default_mqtt_pool() {
        health = health.with_indicator(MqttHealthIndicator::new(mqtt_pool));
    }
    health.build()
}
