//! Building one tenant's runtime, as the tenancy crate's factory port.
//!
//! The composition root implements this and nothing else can: building a
//! tenant's runtime means naming every service, repository and worker the
//! broker has, which is exactly what `andromeda-tenancy` must not see.

use crate::{
    config::runtime::RuntimeConfig,
    tenant_factory::{
        error::TenantAssemblyError,
        runtime::BrokerTenantRuntime,
        services::construct_services,
        storage::initialize_storage,
        tasks::{TenantSignals, supervise_tasks},
        temporal_history::compose_temporal_history,
        transport::construct_transports,
    },
    tenant_state::AndromedaState,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_delivery::mqtt::pool::MqttTransportPool;
use andromeda_entity::{config::EntityConfig, spatial_config::SpatialConfig};
use andromeda_federation::config::FederationConfig;
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_http_subscription::config::NotificationConfig;
use andromeda_jsonld::config::JsonLdConfig;
use andromeda_notification::delivery_config::DeliveryConfig;
use andromeda_postgresql_atomic_write::{
    committer::{EntityInsertMode, PostgresAtomicCommitter},
    config::{AtomicWriteConfig, BulkWriteMode},
};
use andromeda_postgresql_context_store::{config::DatabaseConfig, tenant_database::DatabaseUrl};
use andromeda_snapshot::config::SnapshotConfig;
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_temporal::config::TemporalConfig;
use andromeda_tenancy::{
    config::TenancyConfig,
    database_name::TenantDatabaseName,
    error::TenancyError,
    state_factory::TenantStateFactory,
    tenant_ref::TenantRef,
};
use async_trait::async_trait;
use std::sync::{Arc, OnceLock};
use tracing::debug;
use url::Url;

mod error;
/// Runtime ownership and ordered shutdown.
pub mod runtime;
mod services;
mod storage;
mod subscription_services;
mod tasks;
mod temporal_history;
mod temporal_intent_task;
mod transport;

/// Builds a tenant's runtime from the broker's configuration.
pub struct BrokerTenantFactory {
    /// Owner-provided configuration required to construct one tenant.
    config: Arc<TenantFactoryConfig>,
    /// The URL each tenant's database name is substituted into.
    database_url: DatabaseUrl,
    /// The MQTT pool the unnamed default tenant opened.
    ///
    /// Recorded here because `/health` reports on MQTT delivery and the pool
    /// belongs to a runtime the health router has no handle on. Written once:
    /// the unnamed default is the one tenant that is never evicted.
    default_mqtt_pool: OnceLock<Arc<MqttTransportPool>>,
    /// Process task scope from which every tenant supervisor derives cancellation.
    process_tasks: TaskSupervisor,
    /// The platform route from minted subscription identifiers to tenants.
    subscription_router: Arc<dyn ForwardedSubscriptionRouter>,
}

/// Focused owner configuration retained by tenant construction.
struct TenantFactoryConfig {
    database: DatabaseConfig,
    entity: EntityConfig,
    federation: FederationConfig,
    info: InfoConfig,
    jsonld: JsonLdConfig,
    notifications: NotificationConfig,
    notification_delivery: DeliveryConfig,
    postgresql_write: AtomicWriteConfig,
    snapshot: SnapshotConfig,
    spatial: SpatialConfig,
    temporal: TemporalConfig,
    tenancy: TenancyConfig,
}

impl From<&RuntimeConfig> for TenantFactoryConfig {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            database: config.database.clone(),
            entity: config.entity.clone(),
            federation: config.federation.clone(),
            info: config.info.clone(),
            jsonld: config.jsonld.clone(),
            notifications: config.notifications.clone(),
            notification_delivery: config.notification_delivery.clone(),
            postgresql_write: config.postgresql_write.clone(),
            snapshot: config.snapshot.clone(),
            spatial: config.spatial.clone(),
            temporal: config.temporal.clone(),
            tenancy: config.tenancy.clone(),
        }
    }
}

impl BrokerTenantFactory {
    /// A factory over the broker's configuration.
    pub(super) fn new(
        config: &RuntimeConfig,
        database_url: DatabaseUrl,
        process_tasks: TaskSupervisor,
        subscription_router: Arc<dyn ForwardedSubscriptionRouter>,
    ) -> BrokerTenantFactory {
        BrokerTenantFactory {
            config: Arc::new(TenantFactoryConfig::from(config)),
            database_url,
            default_mqtt_pool: OnceLock::new(),
            process_tasks,
            subscription_router,
        }
    }

    /// The MQTT pool of the default tenant's runtime, once it has been built.
    pub fn default_mqtt_pool(&self) -> Option<Arc<MqttTransportPool>> {
        self.default_mqtt_pool.get().map(Arc::clone)
    }
}

#[async_trait]
impl TenantStateFactory for BrokerTenantFactory {
    type State = AndromedaState;
    type Runtime = BrokerTenantRuntime;

    async fn build(&self, tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<AndromedaState>, BrokerTenantRuntime), TenancyError> {
        let db_url = self.database_url.with_db_name(database);
        let (state, runtime) = build_tenant_state(&self.config, &db_url, tenant, self.process_tasks.tenant(), &self.subscription_router)
            .await
            .map_err(|error| TenancyError::RuntimeUnavailable {
                tenant: tenant.clone(), // clone: the error outlives this borrow
                detail: error.to_string(),
            })?;

        if tenant.is_unnamed_default() && self.default_mqtt_pool.set(Arc::clone(runtime.mqtt_pool())).is_err() {
            debug!("the default tenant's MQTT pool was already recorded; /health keeps reporting on the first");
        }

        Ok((Arc::new(state), runtime))
    }
}

/// Build every service, repository and worker one tenant needs.
///
/// Private to the factory: it is the factory's whole implementation, and the
/// route table has no business calling it.
async fn build_tenant_state(
    config: &TenantFactoryConfig,
    db_url: &Url,
    tenant: &TenantRef,
    task_supervisor: TaskSupervisor,
    subscription_router: &Arc<dyn ForwardedSubscriptionRouter>,
) -> Result<(AndromedaState, BrokerTenantRuntime), TenantAssemblyError> {
    let signals = TenantSignals::new(config.spatial.change_channel_capacity);
    let storage = initialize_storage(config, db_url, tenant).await?;
    let temporal = compose_temporal_history(&storage.pool, &storage.temporal_repo, &config.temporal, &task_supervisor, &storage.live_target)?;
    // Every write path reaches the `current_attribute_values` projection
    // through this complete atomic commit implementation, which writes it in
    // the transaction that mutates the entity.
    let entity_insert_mode = match config.postgresql_write.bulk_mode {
        BulkWriteMode::Unnest => EntityInsertMode::Unnest,
        BulkWriteMode::CopyStaging => EntityInsertMode::CopyStaging,
    };
    let writes: Arc<dyn ContextCommitter> = Arc::new(PostgresAtomicCommitter::new(
        &storage.pool,
        temporal.history_mode,
        entity_insert_mode,
        Arc::clone(&storage.attribute_names),   // clone: the commit interns names through the tenant's one dictionary.
        Arc::clone(&storage.entity_type_names), // clone: the same dictionary, for entity types.
    ));
    let transports = construct_transports()?;
    let services = construct_services(config, &storage, &signals, &transports, &temporal.ports, &writes, subscription_router)?;
    let supervision = supervise_tasks(config, &storage, &services, signals, &writes, &transports.dispatcher, task_supervisor)?;
    Ok((
        services.state,
        BrokerTenantRuntime::new(
            storage.context_workers,
            supervision.handles,
            transports.dispatcher,
            storage.pool,
            supervision.health_shutdown,
            supervision.task_supervisor,
            supervision.drain_wakes,
        ),
    ))
}
