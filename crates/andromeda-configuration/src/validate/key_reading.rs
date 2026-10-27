//! The value a rule reads from a configuration under a key.

use crate::{
    broker_config::BrokerConfig,
    human_byte_size::HumanByteSize,
    human_duration::HumanDuration,
    schema::{database::name_cache::IdleEviction, durable_queue::DurableQueue, management::ManagementMode, temporal::storage::DEFAULT_CHUNK_INTERVAL},
    validate::{
        bound::Quantity,
        config_key::{ConfigKey, QueueKey, QueueName},
    },
};

impl ConfigKey {
    /// The value the key holds in `config` as a rule compares it.
    ///
    /// `None` when the key holds no quantity, when it is not set, or when the
    /// configuration does not read it, such as `management.port` while the
    /// management plane shares the API port. A rule over a key without a
    /// value passes. `temporal.storage.chunk_interval` reads as the interval
    /// in effect, its default when it is not set.
    pub(crate) fn reading(self, config: &BrokerConfig) -> Option<Quantity> {
        match self {
            ConfigKey::ServerPort => Some(count(config.server.port)),
            ConfigKey::ServerGracefulDrainTimeout => Some(span(config.server.graceful_drain_timeout)),
            ConfigKey::ServerAnnouncedCloseTimeout => Some(span(config.server.announced_close_timeout)),
            ConfigKey::ManagementPort => match config.management.mode {
                ManagementMode::SeparatePort => Some(count(config.management.port)),
                ManagementMode::SharedPort | ManagementMode::Disabled => None,
            },
            ConfigKey::ManagementMode
            | ConfigKey::TlsCertificate
            | ConfigKey::TlsPrivateKey
            | ConfigKey::TlsClientAuthenticationCa
            | ConfigKey::TlsClientAuthenticationMode
            | ConfigKey::HttpCorsAllowOrigins
            | ConfigKey::HttpCorsAllowMethods
            | ConfigKey::HttpCorsAllowHeaders
            | ConfigKey::HttpCorsExposeHeaders
            | ConfigKey::HttpCorsCredentials
            | ConfigKey::JsonLdPreloadedContextUrl
            | ConfigKey::TemporalStorageRetentionMode
            | ConfigKey::ObservabilityMetricsMode => None,
            ConfigKey::RequestLimitsTimeout => Some(span(config.request_limits.timeout)),
            ConfigKey::RequestLimitsMaxBody => Some(size(config.request_limits.max_body)),
            ConfigKey::HttpBodyDecodingBlockingThreshold => Some(size(config.http.body_decoding.blocking_threshold)),
            ConfigKey::DatabaseMaxConnections => Some(count(config.database.max_connections)),
            ConfigKey::DatabaseAcquireTimeout => Some(span(config.database.acquire_timeout)),
            ConfigKey::DatabaseNameCacheMaxSizePerDictionary => Some(size(config.database.name_cache.max_size_per_dictionary)),
            ConfigKey::DatabaseNameCacheIdleEvictionAfter => match config.database.name_cache.idle_eviction_after {
                IdleEviction::After(idle) => Some(span(idle)),
                IdleEviction::Never(_) => None,
            },
            ConfigKey::TenancyProvisionalTenantLifetime => Some(span(config.tenancy.provisional_tenant_lifetime)),
            ConfigKey::TenancyResidencyIdleEvictionAfter => Some(span(config.tenancy.residency.idle_eviction_after)),
            ConfigKey::TenancyResidencySweepInterval => Some(span(config.tenancy.residency.sweep_interval)),
            ConfigKey::TenancyResidencyMinWakeInterval => Some(span(config.tenancy.residency.min_wake_interval)),
            ConfigKey::TenancyResidencyWokenLifetime => Some(span(config.tenancy.residency.woken_lifetime)),
            ConfigKey::TenancyBuildAdmissionTimeout => Some(span(config.tenancy.build.admission_timeout)),
            ConfigKey::ExpirySweepInterval => Some(span(config.expiry.sweep_interval)),
            ConfigKey::JsonLdFetchTimeout => Some(span(config.jsonld.fetch.timeout)),
            ConfigKey::JsonLdFetchMaxSize => Some(size(config.jsonld.fetch.max_size)),
            ConfigKey::JsonLdCacheHitFlushInterval => Some(span(config.jsonld.cache.hit_flush_interval)),
            ConfigKey::EntitySpatialIndexRebuildInterval => Some(span(config.entity.spatial_index.rebuild_interval)),
            ConfigKey::EntitySideEffectsDeadLetterAfter => Some(span(config.entity.side_effects.retention.dead_letter_after)),
            ConfigKey::EntitySideEffectsCleanupInterval => Some(span(config.entity.side_effects.retention.cleanup_interval)),
            ConfigKey::Queue(name, key) => Some(queue_reading(queue(config, name), key)),
            ConfigKey::TemporalQueryDefaultLastN => Some(count(config.temporal.query.default_last_n)),
            ConfigKey::TemporalQueryMaxLastN => Some(count(config.temporal.query.max_last_n)),
            ConfigKey::TemporalQueryMaxEdgeGroup => Some(count(config.temporal.query.max_edge_group)),
            ConfigKey::TemporalQueryMaxAggregationInstances => Some(count(config.temporal.query.max_aggregation_instances.get())),
            ConfigKey::TemporalStorageChunkInterval => Some(span(config.temporal.storage.chunk_interval.unwrap_or(DEFAULT_CHUNK_INTERVAL))),
            ConfigKey::TemporalStorageConvertAfter => Some(span(config.temporal.storage.columnstore.convert_after)),
            ConfigKey::TemporalStorageColumnstoreSweepInterval => Some(span(config.temporal.storage.columnstore.sweep_interval)),
            ConfigKey::TemporalStorageDeleteAfter => config.temporal.storage.retention.delete_after.map(span),
            ConfigKey::TemporalStorageRetentionSweepInterval => Some(span(config.temporal.storage.retention.sweep_interval)),
            ConfigKey::NotificationsDeliveryRequestTimeout => Some(span(config.notifications.delivery.request_timeout)),
            ConfigKey::NotificationsDeliveryAcknowledgedAfter => Some(span(config.notifications.delivery.retention.acknowledged_after)),
            ConfigKey::NotificationsDeliveryDeadLetterAfter => Some(span(config.notifications.delivery.retention.dead_letter_after)),
            ConfigKey::NotificationsDeliveryOrphanEventAfter => Some(span(config.notifications.delivery.retention.orphan_event_after)),
            ConfigKey::NotificationsDeliveryCleanupInterval => Some(span(config.notifications.delivery.retention.cleanup_interval)),
            ConfigKey::NotificationsPeriodicMaxPollInterval => Some(span(config.notifications.periodic.max_poll_interval)),
            ConfigKey::FederationForwardingConnectTimeout => Some(span(config.federation.forwarding.connect_timeout)),
            ConfigKey::FederationForwardingRequestTimeout => Some(span(config.federation.forwarding.request_timeout)),
            ConfigKey::SnapshotDefaultLifetime => Some(span(config.snapshot.default_lifetime)),
            ConfigKey::SnapshotMaxLifetime => Some(span(config.snapshot.max_lifetime)),
            ConfigKey::SnapshotMaxStoragePerTenant => config.snapshot.quota.max_storage_per_tenant.map(size),
            ConfigKey::SnapshotPreparationLeaseTimeout => Some(span(config.snapshot.preparation.lease_timeout)),
            ConfigKey::SnapshotPreparationRecoveryInterval => Some(span(config.snapshot.preparation.recovery_interval)),
            ConfigKey::SnapshotPreparationScopedOperationTimeout => Some(span(config.snapshot.preparation.scoped_operation_timeout)),
        }
    }
}

/// The queue table `name` names.
const fn queue(config: &BrokerConfig, name: QueueName) -> &DurableQueue {
    match name {
        QueueName::EntitySideEffects => &config.entity.side_effects.queue,
        QueueName::TemporalHistory => &config.temporal.history.queue,
        QueueName::NotificationDelivery => &config.notifications.delivery.queue,
    }
}

/// The value one key of a queue table holds.
const fn queue_reading(queue: &DurableQueue, key: QueueKey) -> Quantity {
    span(match key {
        QueueKey::PollInterval => queue.poll_interval,
        QueueKey::MaxPollInterval => queue.max_poll_interval,
        QueueKey::LeaseTimeout => queue.lease_timeout,
        QueueKey::RetryInterval => queue.retry_interval,
        QueueKey::MaxRetryInterval => queue.max_retry_interval,
    })
}

/// A duration as a quantity.
const fn span(duration: HumanDuration) -> Quantity {
    Quantity::Duration(duration.duration())
}

/// A byte size as a quantity.
const fn size(size: HumanByteSize) -> Quantity {
    Quantity::Count(size.as_u64())
}

/// A count as a quantity.
fn count(count: impl Into<u64>) -> Quantity {
    Quantity::Count(count.into())
}
