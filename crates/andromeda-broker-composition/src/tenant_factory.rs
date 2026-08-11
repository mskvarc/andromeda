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
        services::{ConstructedServices, ServiceConstructionInputs, construct_services},
        storage::initialize_storage,
        tasks::{TaskSupervisionInputs, TenantSignals, supervise_tasks},
        temporal_intent_task::{TemporalIntentRegistration, register_temporal_intent_drain},
        transport::construct_transports,
    },
    tenant_state::AndromedaState,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_delivery::mqtt::pool::MqttTransportPool;
use andromeda_entity::{batch_activity::BatchActivityTracker, config::EntityConfig, spatial_config::SpatialConfig};
use andromeda_federation::config::FederationConfig;
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_http_subscription::config::NotificationConfig;
use andromeda_jsonld::config::JsonLdConfig;
use andromeda_notification::outbox_config::OutboxConfig;
use andromeda_postgresql_atomic_write::{
    committer::{EntityInsertMode, PostgresAtomicCommitter},
    config::{AtomicWriteConfig, BulkWriteMode},
    history_mode::TemporalHistoryCommitMode,
    intent_store::PostgresTemporalIntentStore,
};
use andromeda_postgresql_context_store::{config::DatabaseConfig, tenant_database::DatabaseUrl};
use andromeda_postgresql_temporal_store::{intent_sink::PostgresTemporalHistorySink, recorder::TemporalRecorder};
use andromeda_snapshot::config::SnapshotConfig;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_temporal::{
    config::{TemporalAppendMode, TemporalConfig},
    pending_overlay::PendingTemporalHistoryOverlay,
    port::{history_reader::TemporalHistoryReader, pending_intent_reader::PendingTemporalIntentReader, temporal_intent_queue::TemporalIntentQueue},
};
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

mod error;
/// Runtime ownership and ordered shutdown.
pub mod runtime;
mod services;
mod storage;
mod tasks;
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
}

/// Focused owner configuration retained by tenant construction.
struct TenantFactoryConfig {
    database: DatabaseConfig,
    entity: EntityConfig,
    federation: FederationConfig,
    info: InfoConfig,
    jsonld: JsonLdConfig,
    notifications: NotificationConfig,
    outbox: OutboxConfig,
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
            outbox: config.outbox.clone(),
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
    pub(super) fn new(config: &RuntimeConfig, database_url: DatabaseUrl, process_tasks: TaskSupervisor) -> BrokerTenantFactory {
        BrokerTenantFactory {
            config: Arc::new(TenantFactoryConfig::from(config)),
            database_url,
            default_mqtt_pool: OnceLock::new(),
            process_tasks,
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
        let (state, runtime) = build_tenant_state(&self.config, &db_url, tenant.clone(), self.process_tasks.tenant()) // clone: the state owns its tenant and the tenant task scope derives from the process scope.
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
    db_url: &url::Url,
    tenant: TenantRef,
    task_supervisor: TaskSupervisor,
) -> Result<(AndromedaState, BrokerTenantRuntime), TenantAssemblyError> {
    let signals = TenantSignals::new(config.spatial.change_channel_capacity);
    let storage = initialize_storage(
        &config.database,
        &config.tenancy,
        &config.jsonld,
        &config.spatial,
        &config.postgresql_write,
        db_url,
        &tenant,
    )
    .await?;
    let pool = storage.pool;
    let engine = storage.engine;
    let context_hit_flush = storage.context_hit_flush;
    let temporal_repo = storage.temporal_repo;
    let entity_ports = storage.entity_ports;
    let registration_store = storage.registration_store;
    let snapshot_store = storage.snapshot_store;
    let subscription_cache = storage.subscription_cache;
    let spatial_index = storage.spatial_index;

    let batch_activity = BatchActivityTracker::new();

    let temporal_recorder = Arc::new(TemporalRecorder::new(Arc::clone(&temporal_repo)));
    let intent_store = Arc::new(PostgresTemporalIntentStore::new(&pool));
    let temporal_sink = Arc::new(PostgresTemporalHistorySink::new(
        Arc::clone(&temporal_repo),
        storage.attribute_current_queue.clone(), // clone: the native committer and deferred sink share one projection producer.
    ));
    let TemporalIntentRegistration {
        publisher: intent_publisher,
        mutation_barrier,
    } = register_temporal_intent_drain(
        &config.temporal,
        &task_supervisor,
        Arc::clone(&intent_store) as Arc<dyn TemporalIntentQueue>,
        Arc::clone(&intent_store) as Arc<dyn PendingTemporalIntentReader>,
        temporal_sink,
    )?;
    let (history_mode, temporal_reader): (TemporalHistoryCommitMode, Arc<dyn TemporalHistoryReader>) = match config.temporal.append_mode {
        TemporalAppendMode::Native => (
            TemporalHistoryCommitMode::Native {
                recorder: Arc::clone(&temporal_recorder), // clone: native commit mode owns its recorder handle.
            },
            Arc::clone(&temporal_repo) as Arc<dyn TemporalHistoryReader>,
        ),
        TemporalAppendMode::Deferred => (
            TemporalHistoryCommitMode::Deferred { publisher: intent_publisher },
            Arc::new(PendingTemporalHistoryOverlay::new(
                Arc::clone(&temporal_repo) as Arc<dyn TemporalHistoryReader>,
                Arc::clone(&intent_store) as Arc<dyn PendingTemporalIntentReader>,
            )),
        ),
    };

    // Sole owner of the materializer producer handle: every write path reaches
    // the projection through this complete atomic commit implementation.
    let entity_insert_mode = match config.postgresql_write.bulk_mode {
        BulkWriteMode::Unnest => EntityInsertMode::Unnest,
        BulkWriteMode::CopyStaging => EntityInsertMode::CopyStaging,
    };
    let writes: Arc<dyn ContextCommitter> = Arc::new(PostgresAtomicCommitter::new(
        &pool,
        history_mode,
        entity_insert_mode,
        storage.attribute_current_queue,
    ));

    let transports = construct_transports()?;
    let ConstructedServices { state, snapshot_preparation } = construct_services(
        ServiceConstructionInputs::builder()
            .database_config(&config.database)
            .entity_config(&config.entity)
            .federation_config(&config.federation)
            .info_config(&config.info)
            .notification_config(&config.notifications)
            .outbox_config(&config.outbox)
            .snapshot_config(&config.snapshot)
            .temporal_config(&config.temporal)
            .tenancy_config(&config.tenancy)
            .pool(&pool)
            .tenant(&tenant)
            .signals(&signals)
            .transports(&transports)
            .engine(&engine)
            .temporal_repo(&temporal_repo)
            .temporal_reader(&temporal_reader)
            .temporal_mutation_barrier(&mutation_barrier)
            .entity_reader(&entity_ports.reader)
            .entity_query_store(&entity_ports.query)
            .entity_discovery_store(&entity_ports.discovery)
            .registration_store(&registration_store)
            .snapshot_store(&snapshot_store)
            .subscription_cache(&subscription_cache)
            .spatial_index(&spatial_index)
            .writes(&writes)
            .batch_activity(&batch_activity)
            .build(),
    )?;
    let entity_policy = Arc::clone(&state.entities); // clone: request handling and expiry scheduling share entity policy.
    let subscription_policy = Arc::clone(&state.subscriptions); // clone: request handling and scheduled expiry share policy.
    let csource_subscription_policy = Arc::clone(&state.csr_subscriptions); // clone: same ownership boundary for CSR subscriptions.
    let forwarding = Arc::clone(&state.forwarding); // clone: request handling and the notification workers share one forwarding service.
    let transport = transports.dispatcher;
    let supervision = supervise_tasks(
        TaskSupervisionInputs::builder()
            .entity_config(&config.entity)
            .info_config(&config.info)
            .outbox_config(&config.outbox)
            .spatial_config(&config.spatial)
            .temporal_config(&config.temporal)
            .pool(&pool)
            .tenant(tenant)
            .signals(signals)
            .materializer(storage.materializer)
            .entity_policy(entity_policy)
            .subscription_policy(subscription_policy)
            .csource_subscription_policy(csource_subscription_policy)
            .snapshot_preparation(snapshot_preparation)
            .entity_reader(entity_ports.reader)
            .entity_query_store(entity_ports.query)
            .entity_discovery_store(entity_ports.discovery)
            .entity_expiry_source(entity_ports.expiry)
            .entity_spatial_snapshots(entity_ports.spatial_snapshots)
            .registration_store(registration_store)
            .forwarding(forwarding)
            .snapshot_store(snapshot_store)
            .subscription_cache(subscription_cache)
            .writes(writes)
            .engine(engine)
            .spatial_index(spatial_index)
            .transport(Arc::clone(&transport))
            .task_supervisor(task_supervisor)
            .build(),
    )?;

    Ok((
        state,
        BrokerTenantRuntime::new(
            context_hit_flush,
            supervision.handles,
            transport,
            pool,
            supervision.health_shutdown,
            supervision.task_supervisor,
            supervision.drain_wakes,
        ),
    ))
}
