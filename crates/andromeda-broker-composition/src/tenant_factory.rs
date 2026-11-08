//! Building one tenant's runtime, as the tenancy crate's factory port.
//!
//! The composition root implements this and nothing else can: building a
//! tenant's runtime means naming every service, repository and worker the
//! broker has, which is exactly what `andromeda-tenancy` must not see.

use crate::{
    configuration::section_admission::database_defaults,
    tenant_factory::{
        background_pacer::{BackgroundPacer, TenantObligations},
        commit_consequence::RuntimeCommitConsequencePublisher,
        error::TenantAssemblyError,
        periodic_due::PeriodicDue,
        runtime::BrokerTenantRuntime,
        services::{TenantServiceCollaborators, construct_services},
        spatial::indexed_commits,
        storage::initialize_storage,
        tasks::{PeriodicPacing, TenantSignals, TenantTaskChannels, snapshot_delivery_signal, supervise_tasks},
        temporal_history::compose_temporal_history,
        transport::construct_dispatcher,
    },
    tenant_state::AndromedaState,
};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    schema::{
        database::DatabaseConfig,
        entity::EntityConfig,
        expiry::ExpiryConfig,
        extensions::ExtensionsConfig,
        federation::FederationConfig,
        jsonld::JsonLdConfig,
        link_field_layout::LinkFieldLayout,
        notifications::NotificationsConfig,
        request_limits::RequestLimitsConfig,
        snapshot::SnapshotConfig,
        temporal::TemporalConfig,
        tenancy::TenancyConfig,
    },
};
use andromeda_context_commit::{commit_consequence_publisher::CommitConsequencePublisher, committer::ContextCommitter};
use andromeda_delivery::mqtt::pool::MqttTransportPool;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_atomic_write::committer::PostgresAtomicCommitter;
use andromeda_postgresql_context_store::{database_defaults::DatabaseDefaults, tenant_extension_versions::PostgresTenantExtensionVersions};
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_task_runtime::{supervisor::TaskSupervisor, wake_trigger::wake_channel};
use andromeda_tenancy::{error::TenancyError, state_factory::TenantStateFactory, tenant_ref::TenantRef};
use async_trait::async_trait;
use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    sync::{Arc, OnceLock},
};
use tracing::{debug, warn};
use url::Url;

mod assembly_invariant;
mod background_pacer;
mod commit_consequence;
mod due_work_check;
mod error;
#[cfg(test)]
mod held_change_fixture;
mod periodic_due;
/// Runtime ownership and ordered shutdown.
pub(crate) mod runtime;
mod services;
mod snapshot_subscriptions;
mod spatial;
mod storage;
mod subscription_services;
mod tasks;
mod temporal_history;
mod temporal_intent_task;
mod transport;

/// Builds a tenant's runtime from the broker's configuration.
pub(crate) struct BrokerTenantFactory {
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
    /// The platform record of the extension versions each opened tenant holds.
    extension_versions: Arc<PostgresTenantExtensionVersions>,
}

/// The tables tenant construction reads, kept for every tenant it builds.
struct TenantFactoryConfig {
    /// How a tenant's database is reached and its caches sized.
    database: DatabaseConfig,
    /// The session defaults reconciled on each tenant database it opens.
    database_defaults: DatabaseDefaults,
    /// What one request may cost, which bounds every write a tenant admits.
    request_limits: RequestLimitsConfig,
    /// How a tenant's expired resources are swept.
    expiry: ExpiryConfig,
    /// Entity batches, their side effects, and the spatial index.
    entity: EntityConfig,
    /// Forwarding to Context Sources and this broker's identity.
    federation: FederationConfig,
    /// How a tenant's `@context` documents are resolved.
    jsonld: JsonLdConfig,
    /// How a tenant's notifications are delivered.
    notifications: NotificationsConfig,
    /// How a tenant's Snapshots are kept and prepared.
    snapshot: SnapshotConfig,
    /// How a tenant's Temporal Evolutions are read, recorded and stored.
    temporal: TemporalConfig,
    /// How tenants are provisioned and kept resident.
    tenancy: TenancyConfig,
    /// Which extensions a tenant serves.
    extensions: ExtensionsConfig,
    /// How every HTTP notification lays out its `Link` field lines, as the
    /// `[http]` table decides for responses.
    link_field: LinkFieldLayout,
    /// The `apiRoot` clients and peers reach the broker at (clause 6.2),
    /// when `server.public_url` configures one.
    public_url: Option<Url>,
    /// The default tenant's alias in effect, from which each tenant's alias
    /// in the `Via` chain follows (Table 5.2.40-1).
    source_alias: ContextSourcePseudonym,
}

impl TenantFactoryConfig {
    /// The sections a tenant is built from, beside the alias in effect.
    fn new(config: &BrokerConfig, source_alias: ContextSourcePseudonym) -> TenantFactoryConfig {
        TenantFactoryConfig {
            database: config.database.clone(),
            database_defaults: database_defaults(config),
            request_limits: config.request_limits,
            expiry: config.expiry,
            entity: config.entity.clone(),
            federation: config.federation.clone(),
            jsonld: config.jsonld.clone(),
            notifications: config.notifications.clone(),
            snapshot: config.snapshot,
            temporal: config.temporal,
            tenancy: config.tenancy.clone(),
            extensions: config.extensions,
            link_field: config.http.link_field,
            public_url: config.server.public_url.clone(),
            source_alias,
        }
    }
}

impl BrokerTenantFactory {
    /// A factory over the broker's configuration.
    pub(super) fn new(
        config: &BrokerConfig,
        source_alias: ContextSourcePseudonym,
        request_url: DatabaseUrl,
        direct_url: DatabaseUrl,
        process_tasks: TaskSupervisor,
        subscription_router: Arc<dyn ForwardedSubscriptionRouter>,
        extension_versions: Arc<PostgresTenantExtensionVersions>,
    ) -> BrokerTenantFactory {
        BrokerTenantFactory {
            config: Arc::new(TenantFactoryConfig::new(config, source_alias)),
            request_url,
            direct_url,
            default_mqtt_pool: OnceLock::new(),
            process_tasks,
            subscription_router,
            extension_versions,
        }
    }

    /// The MQTT pool of the default tenant's runtime, once it has been built.
    pub(crate) fn default_mqtt_pool(&self) -> Option<Arc<MqttTransportPool>> {
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
        .map_err(|error| error.into_tenancy_error(tenant))?;

        if tenant.is_unnamed_default() && self.default_mqtt_pool.set(Arc::clone(runtime.mqtt_pool())).is_err() {
            debug!("the default tenant's MQTT pool was already recorded; /health keeps reporting on the first");
        }
        // The record only steers which dormant tenants a wake pass brings up to
        // date, so a failed write costs at most one more load and never the open.
        if let Err(error) = self.extension_versions.record(database, runtime.pool()).await {
            warn!(error = %error, event = "tenant_extension_versions_unrecorded", "the tenant's extension versions could not be recorded");
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
    let signals = TenantSignals::new(config.notifications.evaluation.max_queued_changes);
    let storage = initialize_storage(config, serving_url, migration_url, tenant).await?;
    // One pacer for every background pass the tenant runs, created before the
    // first of them is registered, so one obligation read decides for all of
    // them while the tenant is quiet.
    let pacer = Arc::new(BackgroundPacer::new(
        config.expiry.sweep_interval.duration(),
        Arc::new(TenantObligations::new(&storage.pool)),
    ));
    let periodic_due = Arc::new(PeriodicDue::default());
    let temporal = compose_temporal_history(
        &storage.pool,
        &storage.temporal_repo,
        &config.temporal.history,
        config.request_limits.timeout.duration(),
        &task_supervisor,
        &pacer,
    )?;
    // Every write path reaches the `current_attributes` rows through this
    // complete atomic commit implementation, which writes them in the
    // transaction that mutates the entity.
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
    let snapshot_delivery_signal = snapshot_delivery_signal(snapshot_status_wake.clone());
    let consequence_publisher: Arc<dyn CommitConsequencePublisher> =
        Arc::new(RuntimeCommitConsequencePublisher::new(committed_notification_wake, side_effect_wake));
    let writes: Arc<dyn ContextCommitter> = Arc::new(PostgresAtomicCommitter::new(
        &storage.pool,
        temporal.history_mode,
        config.temporal.history.core_writes,
        Arc::clone(&storage.attribute_names),
        Arc::clone(&storage.entity_type_names),
        consequence_publisher,
    ));
    let writes = indexed_commits(writes, &storage.spatial, tenant)?;
    let dispatcher = construct_dispatcher(&config.notifications.delivery, config.link_field)?;
    let services = construct_services(
        config,
        &storage,
        &signals,
        &dispatcher,
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
        pacer: Arc::clone(&pacer),
        periodic: PeriodicPacing {
            ceiling: config.notifications.periodic.max_poll_interval.duration(),
            pacer: Arc::clone(&pacer),
            due: Arc::clone(&periodic_due),
        },
    };
    let supervision = supervise_tasks(config, &storage, &services, &writes, &dispatcher, task_supervisor, channels)?;
    Ok((
        services.state,
        BrokerTenantRuntime::new(storage.context_workers, supervision, dispatcher, storage.pool, pacer, periodic_due),
    ))
}
