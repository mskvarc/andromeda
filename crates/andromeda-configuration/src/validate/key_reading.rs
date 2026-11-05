//! The value a rule reads from a configuration under a key.

use crate::{
    broker_config::BrokerConfig,
    human_byte_size::HumanByteSize,
    human_duration::HumanDuration,
    schema::{durable_queue::DurableQueue, management::ManagementMode, temporal::storage::DEFAULT_CHUNK_INTERVAL},
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
            ConfigKey::TlsHandshakeTimeout => Some(span(config.tls.handshake_timeout)),
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
            | ConfigKey::HttpForwardedTrust
            | ConfigKey::HttpForwardedProxies
            | ConfigKey::HttpCompressionMode
            | ConfigKey::HttpCompressionAlgorithms
            | ConfigKey::HttpDecompressionMode
            | ConfigKey::HttpDecompressionAlgorithms
            | ConfigKey::JsonLdPreloadedContextUrl
            | ConfigKey::TemporalStorageRetentionMode
            | ConfigKey::FederationCooldownMode
            | ConfigKey::ObservabilityMetricsPrometheusMode => None,
            ConfigKey::RequestLimitsTimeout => Some(span(config.request_limits.timeout)),
            ConfigKey::RequestLimitsMaxBody => Some(size(config.request_limits.max_body)),
            ConfigKey::RequestLimitsMaxHeaderSection => Some(Quantity::Bytes(u64::from(config.request_limits.max_header_section.octets()))),
            ConfigKey::RequestLimitsMaxAttributeNesting => Some(width(config.request_limits.max_attribute_nesting.get())),
            ConfigKey::RequestLimitsMaxQNesting => Some(width(config.request_limits.max_q_nesting.get())),
            ConfigKey::HttpBodyDecodingBlockingThreshold => Some(size(config.http.body_decoding.blocking_threshold)),
            ConfigKey::DatabasePoolerMaxConnectionsPerTenant => config.database.pooler.as_ref().map(|pooler| count(pooler.max_connections_per_tenant.get())),
            ConfigKey::DatabaseWriteReservePercent => Some(count(u8::from(config.database.write_reserve_percent))),
            ConfigKey::DatabaseAcquireTimeout => Some(span(config.database.acquire_timeout)),
            ConfigKey::DatabaseNameCacheMaxSizePerDictionary => Some(size(config.database.name_cache.max_size_per_dictionary)),
            ConfigKey::DatabaseNameCacheIdleEvictionAfter => config.database.name_cache.idle_eviction_after.limit().map(span),
            ConfigKey::DatabaseMaintenanceRepackMaxRelationSize => config.database.maintenance.repack.max_relation_size.limit().map(size),
            ConfigKey::TenancyProvisionalTenantLifetime => Some(span(config.tenancy.provisional_tenant_lifetime)),
            ConfigKey::TenancyDatabasesMaxConnections => Some(count(config.tenancy.databases.max_connections.get())),
            ConfigKey::TenancyResidencyIdleEvictionAfter => Some(span(config.tenancy.residency.idle_eviction_after)),
            ConfigKey::TenancyResidencySweepInterval => Some(span(config.tenancy.residency.sweep_interval)),
            ConfigKey::TenancyResidencyMinWakeInterval => Some(span(config.tenancy.residency.min_wake_interval)),
            ConfigKey::TenancyResidencyWokenLifetime => Some(span(config.tenancy.residency.woken_lifetime)),
            ConfigKey::TenancyBuildAdmissionTimeout => Some(span(config.tenancy.build.admission_timeout)),
            ConfigKey::ExpirySweepInterval => Some(span(config.expiry.sweep_interval)),
            ConfigKey::JsonLdFetchTimeout => Some(span(config.jsonld.fetch.timeout)),
            ConfigKey::JsonLdFetchMaxSize => Some(size(config.jsonld.fetch.max_size)),
            ConfigKey::JsonLdCacheHitFlushInterval => Some(span(config.jsonld.cache.hit_flush_interval)),
            ConfigKey::JsonLdCacheDefaultLifetime => Some(Quantity::Duration(config.jsonld.cache.default_lifetime.as_duration())),
            ConfigKey::JsonLdCacheHostedRevalidationInterval => Some(Quantity::Duration(config.jsonld.cache.hosted_revalidation_interval.as_duration())),
            ConfigKey::EntitySpatialIndexRebuildInterval => Some(span(config.entity.spatial_index.rebuild_interval)),
            ConfigKey::EntitySideEffectsDeadLetterAfter => Some(span(config.entity.side_effects.retention.dead_letter_after)),
            ConfigKey::EntitySideEffectsRetentionSweepInterval => Some(span(config.entity.side_effects.retention.sweep_interval)),
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
            ConfigKey::NotificationsEvaluationShutdownDrain => Some(span(config.notifications.evaluation.shutdown_drain)),
            ConfigKey::NotificationsEvaluationSubscriptionReconcileInterval => Some(span(config.notifications.evaluation.subscription_reconcile_interval)),
            ConfigKey::NotificationsDeliveryMaxHttpBody => Some(size(config.notifications.delivery.max_http_body.byte_size())),
            ConfigKey::NotificationsDeliveryMaxAttempts => Some(count(config.notifications.delivery.max_attempts.get())),
            ConfigKey::NotificationsDeliveryMaxMqttPacket => Some(Quantity::Bytes(u64::from(config.notifications.delivery.max_mqtt_packet.octets()))),
            ConfigKey::NotificationsDeliveryMaxConcurrent => Some(count(config.notifications.delivery.max_concurrent.get())),
            ConfigKey::NotificationsDeliveryMaxErrorLength => Some(count(config.notifications.delivery.max_error_length.get())),
            ConfigKey::NotificationsDeliveryAcknowledgedAfter => Some(span(config.notifications.delivery.retention.acknowledged_after)),
            ConfigKey::NotificationsDeliveryDeadLetterAfter => Some(span(config.notifications.delivery.retention.dead_letter_after)),
            ConfigKey::NotificationsDeliveryOrphanEventAfter => Some(span(config.notifications.delivery.retention.orphan_event_after)),
            ConfigKey::NotificationsDeliveryRetentionSweepInterval => Some(span(config.notifications.delivery.retention.sweep_interval)),
            ConfigKey::NotificationsPeriodicMaxPollInterval => Some(span(config.notifications.periodic.max_poll_interval)),
            ConfigKey::FederationForwardingConnectTimeout => Some(span(config.federation.forwarding.connect_timeout)),
            ConfigKey::FederationForwardingRequestTimeout => Some(span(config.federation.forwarding.request_timeout)),
            ConfigKey::FederationQueryMaxEntities => Some(count(config.federation.query.max_entities.get())),
            ConfigKey::FederationQueryMaxSourceRows => Some(count(config.federation.query.max_source_rows.get())),
            ConfigKey::FederationCooldownRetryInterval => Some(span(config.federation.cooldown.retry_interval)),
            ConfigKey::FederationCooldownMaxRetryInterval => Some(span(config.federation.cooldown.max_retry_interval)),
            ConfigKey::SnapshotDefaultLifetime => Some(span(config.snapshot.default_lifetime)),
            ConfigKey::SnapshotMaxLifetime => Some(span(config.snapshot.max_lifetime)),
            ConfigKey::SnapshotMaxStoragePerTenant => config.snapshot.quota.max_storage_per_tenant.limit().map(size),
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
    Quantity::Bytes(size.as_u64())
}

/// A count held in a `usize` as a quantity, saturating on a platform whose
/// address width passes 64 bits.
fn width(count: usize) -> Quantity {
    Quantity::Count(u64::try_from(count).unwrap_or(u64::MAX))
}

/// A count as a quantity.
fn count(count: impl Into<u64>) -> Quantity {
    Quantity::Count(count.into())
}
