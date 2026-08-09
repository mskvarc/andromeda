//! Building one tenant's runtime, as the tenancy crate's factory port.
//!
//! The composition root implements this and nothing else can: building a
//! tenant's runtime means naming every service, repository and worker the
//! broker has, which is exactly what `andromeda-tenancy` must not see.

use crate::{
    api::{
        context_hit_flush::ContextHitFlushWorker,
        state::AndromedaState,
        tenant_factory::{
            runtime::BrokerTenantRuntime,
            services::{ServiceConstructionInputs, construct_services},
            storage::initialize_storage,
            transport::construct_transports,
            workers::{TenantSignals, WorkerSupervisionInputs, supervise_workers},
        },
    },
    batch_activity::BatchActivityTracker,
    config::runtime::RuntimeConfig,
    error::AndromedaError,
    repository::atomic_write::anchor::PostgresAnchorStore,
    service::entity::write_coordinator::{AnchoredWriteCoordinator, WriteCoordinator},
};
use andromeda_delivery::{dispatch::TransportDispatcher, mqtt::pool::MqttTransportPool};
use andromeda_postgresql_context_store::tenant_database::DatabaseUrl;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_tenancy::{database_name::TenantDatabaseName, error::TenancyError, state_factory::TenantStateFactory, tenant_ref::TenantRef};
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::{Arc, OnceLock};
use tokio::task::JoinHandle;
use tracing::debug;

/// Runtime ownership and ordered shutdown.
pub mod runtime;
mod services;
mod storage;
mod transport;
mod workers;

/// Builds a tenant's runtime from the broker's configuration.
pub struct BrokerTenantFactory {
    /// Every tunable a tenant's services read.
    config: Arc<RuntimeConfig>,
    /// The URL each tenant's database name is substituted into.
    database_url: DatabaseUrl,
    /// The MQTT pool the unnamed default tenant opened.
    ///
    /// Recorded here because `/health` reports on MQTT delivery and the pool
    /// belongs to a runtime the health router has no handle on. Written once:
    /// the unnamed default is the one tenant that is never evicted.
    default_mqtt_pool: OnceLock<Arc<MqttTransportPool>>,
}

impl BrokerTenantFactory {
    /// A factory over the broker's configuration.
    pub const fn new(config: Arc<RuntimeConfig>, database_url: DatabaseUrl) -> BrokerTenantFactory {
        BrokerTenantFactory {
            config,
            database_url,
            default_mqtt_pool: OnceLock::new(),
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
        let (state, context_hit_flush, worker_handles, transport, pool, health_shutdown) = build_tenant_state(&self.config, &db_url, tenant.clone()) // clone: the state owns its tenant
            .await
            .map_err(|error| TenancyError::RuntimeUnavailable {
                tenant: tenant.clone(), // clone: the error outlives this borrow
                detail: error.to_string(),
            })?;

        if tenant.is_unnamed_default() && self.default_mqtt_pool.set(Arc::clone(transport.mqtt_pool())).is_err() {
            debug!("the default tenant's MQTT pool was already recorded; /health keeps reporting on the first");
        }

        Ok((
            Arc::new(state),
            BrokerTenantRuntime::new(context_hit_flush, worker_handles, transport, pool, health_shutdown),
        ))
    }
}

/// Build every service, repository and worker one tenant needs.
///
/// Private to the factory: it is the factory's whole implementation, and the
/// route table has no business calling it.
async fn build_tenant_state(
    config: &RuntimeConfig,
    db_url: &url::Url,
    tenant: TenantRef,
) -> Result<
    (
        AndromedaState,
        Option<ContextHitFlushWorker>,
        Vec<JoinHandle<()>>,
        Arc<TransportDispatcher>,
        PgPool,
        Option<tokio::sync::watch::Sender<bool>>,
    ),
    AndromedaError,
> {
    let signals = TenantSignals::new(config.spatial.change_channel_capacity);
    let storage = initialize_storage(config, db_url, &tenant).await?;
    let pool = storage.pool;
    let engine = storage.engine;
    let context_hit_flush = storage.context_hit_flush;
    let temporal_repo = storage.temporal_repo;
    let entity_store = storage.entity_store;
    let registration_store = storage.registration_store;
    let snapshot_store = storage.snapshot_store;
    let subscription_cache = storage.subscription_cache;
    let spatial_index = storage.spatial_index;

    let batch_activity = BatchActivityTracker::new();

    let temporal_recorder = Arc::new(TemporalRecorder::new(Arc::clone(&temporal_repo)));

    // Sole owner of the materializer producer handle: every write path reaches
    // the projection through an atomic write, so no other service needs one.
    // The anchor also owns the bulk-write mode, which is a statement shape
    // rather than a caller's choice.
    let writes: Arc<dyn WriteCoordinator> = Arc::new(AnchoredWriteCoordinator::new(PostgresAnchorStore::new(
        &pool,
        Arc::clone(&temporal_recorder),
        config.performance.bulk_write_mode,
        storage.attribute_current_queue,
    )));

    let transports = construct_transports()?;
    let state = construct_services(
        ServiceConstructionInputs::builder()
            .config(config)
            .pool(&pool)
            .tenant(&tenant)
            .signals(&signals)
            .transports(&transports)
            .engine(&engine)
            .temporal_repo(&temporal_repo)
            .temporal_recorder(&temporal_recorder)
            .entity_store(&entity_store)
            .registration_store(&registration_store)
            .snapshot_store(&snapshot_store)
            .subscription_cache(&subscription_cache)
            .spatial_index(&spatial_index)
            .writes(&writes)
            .batch_activity(&batch_activity)
            .build(),
    )?;
    let transport = transports.dispatcher;
    let supervision = supervise_workers(
        WorkerSupervisionInputs::builder()
            .config(config)
            .pool(&pool)
            .tenant(tenant)
            .signals(signals)
            .materializer(storage.materializer)
            .entity_store(entity_store)
            .registration_store(registration_store)
            .snapshot_store(snapshot_store)
            .subscription_cache(subscription_cache)
            .writes(writes)
            .engine(engine)
            .spatial_index(spatial_index)
            .batch_activity(batch_activity)
            .transport(Arc::clone(&transport))
            .build(),
    );

    Ok((state, context_hit_flush, supervision.handles, transport, pool, supervision.health_shutdown))
}
