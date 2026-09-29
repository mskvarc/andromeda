//! Building one tenant's runtime, as the tenancy crate's factory port.
//!
//! The composition root implements this and nothing else can: building a
//! tenant's runtime means naming every service, repository and worker the
//! broker has, which is exactly what `andromeda-tenancy` must not see.

use crate::{
    config::{notifications::NotificationsConfig, runtime::RuntimeConfig},
    tenant_factory::{
        background_pacer::{BackgroundPacer, TenantObligations},
        commit_consequence::RuntimeCommitConsequencePublisher,
        error::TenantAssemblyError,
        runtime::BrokerTenantRuntime,
        services::{TenantServiceCollaborators, construct_services},
        spatial::indexed_commits,
        storage::initialize_storage,
        tasks::{TenantSignals, TenantTaskChannels, snapshot_delivery_signal, supervise_tasks},
        temporal_history::compose_temporal_history,
        transport::construct_transports,
    },
    tenant_state::AndromedaState,
};
use andromeda_context_commit::{commit_consequence_publisher::CommitConsequencePublisher, committer::ContextCommitter};
use andromeda_delivery::mqtt::pool::MqttTransportPool;
use andromeda_entity::{config::EntityConfig, spatial_config::SpatialConfig};
use andromeda_extensions::config::ExtensionsConfig;
use andromeda_federation::config::FederationConfig;
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_jsonld::config::JsonLdConfig;
use andromeda_postgresql_atomic_write::{
    committer::{EntityInsertMode, PostgresAtomicCommitter},
    config::{AtomicWriteConfig, BulkWriteMode},
};
use andromeda_postgresql_context_store::{config::DatabaseConfig, tenant_database::DatabaseUrl};
use andromeda_snapshot::config::SnapshotConfig;
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_task_runtime::{supervisor::TaskSupervisor, wake_trigger::wake_channel};
use andromeda_temporal::config::TemporalConfig;
use andromeda_tenancy::{
    config::TenancyConfig,
    database_name::TenantDatabaseName,
    error::TenancyError,
    state_factory::TenantStateFactory,
    tenant_ref::TenantRef,
};
use async_trait::async_trait;
use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    sync::{Arc, OnceLock},
};
use tracing::debug;
use url::Url;

mod background_pacer;
mod commit_consequence;
mod error;
/// Runtime ownership and ordered shutdown.
pub mod runtime;
mod services;
mod spatial;
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
    /// The request-path base URL each tenant's serving pool is derived from.
    /// Reaches PostgreSQL through the pooler when one is configured.
    request_url: DatabaseUrl,
    /// The straight-to-PostgreSQL base URL each tenant's migration is derived
    /// from. Never a pooler: the migrator holds a session advisory lock.
    direct_url: DatabaseUrl,
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
    identity: InfoConfig,
    jsonld: JsonLdConfig,
    notifications: NotificationsConfig,
    atomic_write: AtomicWriteConfig,
    snapshot: SnapshotConfig,
    spatial: SpatialConfig,
    temporal: TemporalConfig,
    tenancy: TenancyConfig,
    extensions: ExtensionsConfig,
}

impl From<&RuntimeConfig> for TenantFactoryConfig {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            database: config.database.clone(),
            entity: config.entity.clone(),
            federation: config.federation.clone(),
            identity: config.identity.clone(),
            jsonld: config.jsonld.clone(),
            notifications: config.notifications.clone(),
            atomic_write: config.atomic_write.clone(),
            snapshot: config.snapshot.clone(),
            spatial: config.spatial.clone(),
            temporal: config.temporal.clone(),
            tenancy: config.tenancy.clone(),
            extensions: config.extensions.clone(),
        }
    }
}

impl BrokerTenantFactory {
    /// A factory over the broker's configuration.
    pub(super) fn new(
        config: &RuntimeConfig,
        request_url: DatabaseUrl,
        direct_url: DatabaseUrl,
        process_tasks: TaskSupervisor,
        subscription_router: Arc<dyn ForwardedSubscriptionRouter>,
    ) -> BrokerTenantFactory {
        BrokerTenantFactory {
            config: Arc::new(TenantFactoryConfig::from(config)),
            request_url,
            direct_url,
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
        let serving_url = self.request_url.with_database_name(database);
        let migration_url = self.direct_url.with_database_name(database);
        let (state, runtime) = build_tenant_state(
            &self.config,
            &serving_url,
            &migration_url,
            tenant,
            self.process_tasks.tenant(tenant_jitter_seed(tenant)),
            &self.subscription_router,
        )
        .await
        .map_err(|error| {
            // A saturated backend pool is backpressure, shed as a 503; only a
            // genuine construction fault is the internal RuntimeUnavailable.
            if error.is_saturation() {
                TenancyError::Saturated { tenant: tenant.clone() } // clone: the error outlives this borrow
            } else {
                TenancyError::RuntimeUnavailable {
                    tenant: tenant.clone(), // clone: the error outlives this borrow
                    detail: error.to_string(),
                }
            }
        })?;

        if tenant.is_unnamed_default() && self.default_mqtt_pool.set(Arc::clone(runtime.mqtt_pool())).is_err() {
            debug!("the default tenant's MQTT pool was already recorded; /health keeps reporting on the first");
        }

        Ok((Arc::new(state), runtime))
    }
}

/// A stable per-tenant seed that spreads a tenant's scheduled background tasks
/// away from every other tenant's, so a burst of freshly provisioned tenants does
/// not run every shared task in lockstep and stampede the database. The unnamed
/// default is a singleton that never bursts, so it takes a fixed zero seed.
fn tenant_jitter_seed(tenant: &TenantRef) -> u64 {
    match tenant.tenant_name() {
        None => 0,
        Some(name) => {
            let mut hasher = DefaultHasher::new();
            hasher.write(name.as_str().as_bytes());
            hasher.finish()
        }
    }
}

/// Build every service, repository and worker one tenant needs.
///
/// Private to the factory: it is the factory's whole implementation, and the
/// route table has no business calling it.
async fn build_tenant_state(
    config: &TenantFactoryConfig,
    serving_url: &Url,
    migration_url: &Url,
    tenant: &TenantRef,
    task_supervisor: TaskSupervisor,
    subscription_router: &Arc<dyn ForwardedSubscriptionRouter>,
) -> Result<(AndromedaState, BrokerTenantRuntime), TenantAssemblyError> {
    // Only live delivery reads committed entity changes from the channel; a
    // durable tenant still publishes into it and needs no more than one slot.
    let entity_change_capacity = if config.notifications.delivery.is_durable() {
        1
    } else {
        config.notifications.live_change_capacity
    };
    let signals = TenantSignals::new(entity_change_capacity);
    let storage = initialize_storage(config, serving_url, migration_url, tenant).await?;
    // One pacer for every background pass the tenant runs, created before the
    // first of them is registered, so one obligation read decides for all of
    // them while the tenant is quiet.
    let pacer = Arc::new(BackgroundPacer::new(
        config.temporal.expiration_check_interval.duration(),
        Arc::new(TenantObligations::new(&storage.pool)),
    ));
    let temporal = compose_temporal_history(&storage.pool, &storage.temporal_repo, &config.temporal, &task_supervisor, &pacer)?;
    // Every write path reaches the `current_attribute_values` projection
    // through this complete atomic commit implementation, which writes it in
    // the transaction that mutates the entity.
    let entity_insert_mode = match config.atomic_write.bulk_mode {
        BulkWriteMode::Unnest => EntityInsertMode::Unnest,
        BulkWriteMode::CopyStaging => EntityInsertMode::CopyStaging,
    };
    // The consequence drains' wake channels are created here, before the drains
    // are registered, so the committer can hold their wake handles while the
    // drains are still being assembled; a commit that races assembly and wakes
    // through a retained handle is not lost, and the drain reads it on its first
    // pass. The receivers are handed to registration below. The
    // notification-delivery drain runs under either delivery mode, for the
    // deliveries the tenant database already queues; only a durable commit
    // queues new ones, so only a durable committer holds its wake handle.
    let (side_effect_wake, side_effect_receiver) = wake_channel();
    let (notification_wake, notification_receiver) = wake_channel();
    let committed_notification_wake = config.notifications.delivery.is_durable().then_some(notification_wake);
    // The Snapshot status-delivery drain's wake channel follows the same
    // pattern: created before the service and drain exist so the create-time and
    // update paths can hold a wake handle while the drain is still assembling. A
    // wake raised before the drain's first read is retained by the one-slot
    // channel, so a Snapshot created the instant a tenant loads is not left to
    // the drain's idle fallback.
    let (snapshot_status_wake, snapshot_status_receiver) = wake_channel();
    let snapshot_delivery_signal = snapshot_delivery_signal(snapshot_status_wake.clone()); // clone: the sweep keeps the handle while the service holds its own.
    let consequence_publisher: Arc<dyn CommitConsequencePublisher> =
        Arc::new(RuntimeCommitConsequencePublisher::new(committed_notification_wake, side_effect_wake));
    let writes: Arc<dyn ContextCommitter> = Arc::new(PostgresAtomicCommitter::new(
        &storage.pool,
        temporal.history_mode,
        config.temporal.core_write_history,
        entity_insert_mode,
        Arc::clone(&storage.attribute_names),   // clone: the commit interns names through the tenant's one dictionary.
        Arc::clone(&storage.entity_type_names), // clone: the same dictionary, for entity types.
        consequence_publisher,
    ));
    let writes = indexed_commits(writes, &storage.spatial, tenant)?;
    let transports = construct_transports(config.notifications.delivery.request_timeout())?;
    let services = construct_services(
        config,
        &storage,
        &signals,
        &transports,
        &temporal.ports,
        TenantServiceCollaborators {
            writes: &writes,
            subscription_router,
            snapshot_delivery_signal,
        },
    )?;
    let channels = TenantTaskChannels {
        signals,
        notification_wake: notification_receiver,
        side_effect_wake: side_effect_receiver,
        snapshot_status_wake,
        snapshot_status_receiver,
        pacer: Arc::clone(&pacer), // clone: the tasks consult the pacer the runtime reports use to.
    };
    let supervision = supervise_tasks(config, &storage, &services, &writes, &transports.dispatcher, task_supervisor, channels)?;
    Ok((
        services.state,
        BrokerTenantRuntime::new(
            storage.context_workers,
            supervision.handles,
            transports.dispatcher,
            storage.pool,
            supervision.health_shutdown,
            supervision.task_supervisor,
            pacer,
        ),
    ))
}
