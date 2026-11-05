//! The key paths a configuration rule names.
//!
//! A violation names the keys it is about by these values rather than by text,
//! so a rule and its test agree on a key by type, and the dotted path an
//! operator reads is spelled once.

use derive_more::Display;

/// A durable queue table, named by its key path.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum QueueName {
    /// `entity.side_effects.queue`.
    #[display("entity.side_effects.queue")]
    EntitySideEffects,
    /// `temporal.history.queue`.
    #[display("temporal.history.queue")]
    TemporalHistory,
    /// `notifications.delivery.queue`.
    #[display("notifications.delivery.queue")]
    NotificationDelivery,
}

/// A key of a durable queue table that a rule names.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display(rename_all = "snake_case")]
pub enum QueueKey {
    /// `poll_interval`.
    PollInterval,
    /// `max_poll_interval`.
    MaxPollInterval,
    /// `lease_timeout`.
    LeaseTimeout,
    /// `retry_interval`.
    RetryInterval,
    /// `max_retry_interval`.
    MaxRetryInterval,
}

/// A configuration key a rule names, displayed as its dotted path.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ConfigKey {
    /// `server.port`.
    #[display("server.port")]
    ServerPort,
    /// `server.graceful_drain_timeout`.
    #[display("server.graceful_drain_timeout")]
    ServerGracefulDrainTimeout,
    /// `server.announced_close_timeout`.
    #[display("server.announced_close_timeout")]
    ServerAnnouncedCloseTimeout,
    /// `management.port`.
    #[display("management.port")]
    ManagementPort,
    /// `management.mode`.
    #[display("management.mode")]
    ManagementMode,
    /// `tls.certificate`.
    #[display("tls.certificate")]
    TlsCertificate,
    /// `tls.private_key`.
    #[display("tls.private_key")]
    TlsPrivateKey,
    /// `tls.handshake_timeout`.
    #[display("tls.handshake_timeout")]
    TlsHandshakeTimeout,
    /// `tls.client_authentication.ca`.
    #[display("tls.client_authentication.ca")]
    TlsClientAuthenticationCa,
    /// `tls.client_authentication.mode`.
    #[display("tls.client_authentication.mode")]
    TlsClientAuthenticationMode,
    /// `request_limits.timeout`.
    #[display("request_limits.timeout")]
    RequestLimitsTimeout,
    /// `request_limits.max_body`.
    #[display("request_limits.max_body")]
    RequestLimitsMaxBody,
    /// `request_limits.max_header_section`.
    #[display("request_limits.max_header_section")]
    RequestLimitsMaxHeaderSection,
    /// `request_limits.max_attribute_nesting`.
    #[display("request_limits.max_attribute_nesting")]
    RequestLimitsMaxAttributeNesting,
    /// `request_limits.max_q_nesting`.
    #[display("request_limits.max_q_nesting")]
    RequestLimitsMaxQNesting,
    /// `http.body_decoding.blocking_threshold`.
    #[display("http.body_decoding.blocking_threshold")]
    HttpBodyDecodingBlockingThreshold,
    /// `http.cors.allow_origins`.
    #[display("http.cors.allow_origins")]
    HttpCorsAllowOrigins,
    /// `http.cors.allow_methods`.
    #[display("http.cors.allow_methods")]
    HttpCorsAllowMethods,
    /// `http.cors.allow_headers`.
    #[display("http.cors.allow_headers")]
    HttpCorsAllowHeaders,
    /// `http.cors.expose_headers`.
    #[display("http.cors.expose_headers")]
    HttpCorsExposeHeaders,
    /// `http.cors.credentials`.
    #[display("http.cors.credentials")]
    HttpCorsCredentials,
    /// `http.compression.mode`.
    #[display("http.compression.mode")]
    HttpCompressionMode,
    /// `http.compression.algorithms`.
    #[display("http.compression.algorithms")]
    HttpCompressionAlgorithms,
    /// `http.decompression.mode`.
    #[display("http.decompression.mode")]
    HttpDecompressionMode,
    /// `http.decompression.algorithms`.
    #[display("http.decompression.algorithms")]
    HttpDecompressionAlgorithms,
    /// `http.forwarded.trust`.
    #[display("http.forwarded.trust")]
    HttpForwardedTrust,
    /// `http.forwarded.proxies`.
    #[display("http.forwarded.proxies")]
    HttpForwardedProxies,
    /// `database.pooler.max_connections_per_tenant`.
    #[display("database.pooler.max_connections_per_tenant")]
    DatabasePoolerMaxConnectionsPerTenant,
    /// `database.write_reserve_percent`.
    #[display("database.write_reserve_percent")]
    DatabaseWriteReservePercent,
    /// `database.acquire_timeout`.
    #[display("database.acquire_timeout")]
    DatabaseAcquireTimeout,
    /// `database.name_cache.max_size_per_dictionary`.
    #[display("database.name_cache.max_size_per_dictionary")]
    DatabaseNameCacheMaxSizePerDictionary,
    /// `database.name_cache.idle_eviction_after`.
    #[display("database.name_cache.idle_eviction_after")]
    DatabaseNameCacheIdleEvictionAfter,
    /// `database.maintenance.repack.max_relation_size`.
    #[display("database.maintenance.repack.max_relation_size")]
    DatabaseMaintenanceRepackMaxRelationSize,
    /// `tenancy.provisional_tenant_lifetime`.
    #[display("tenancy.provisional_tenant_lifetime")]
    TenancyProvisionalTenantLifetime,
    /// `tenancy.databases.max_connections`.
    #[display("tenancy.databases.max_connections")]
    TenancyDatabasesMaxConnections,
    /// `tenancy.residency.idle_eviction_after`.
    #[display("tenancy.residency.idle_eviction_after")]
    TenancyResidencyIdleEvictionAfter,
    /// `tenancy.residency.sweep_interval`.
    #[display("tenancy.residency.sweep_interval")]
    TenancyResidencySweepInterval,
    /// `tenancy.residency.min_wake_interval`.
    #[display("tenancy.residency.min_wake_interval")]
    TenancyResidencyMinWakeInterval,
    /// `tenancy.residency.woken_lifetime`.
    #[display("tenancy.residency.woken_lifetime")]
    TenancyResidencyWokenLifetime,
    /// `tenancy.build.admission_timeout`.
    #[display("tenancy.build.admission_timeout")]
    TenancyBuildAdmissionTimeout,
    /// `expiry.sweep_interval`.
    #[display("expiry.sweep_interval")]
    ExpirySweepInterval,
    /// `jsonld.fetch.timeout`.
    #[display("jsonld.fetch.timeout")]
    JsonLdFetchTimeout,
    /// `jsonld.fetch.max_size`.
    #[display("jsonld.fetch.max_size")]
    JsonLdFetchMaxSize,
    /// `jsonld.cache.hit_flush_interval`.
    #[display("jsonld.cache.hit_flush_interval")]
    JsonLdCacheHitFlushInterval,
    /// `jsonld.cache.default_lifetime`.
    #[display("jsonld.cache.default_lifetime")]
    JsonLdCacheDefaultLifetime,
    /// `jsonld.cache.hosted_revalidation_interval`.
    #[display("jsonld.cache.hosted_revalidation_interval")]
    JsonLdCacheHostedRevalidationInterval,
    /// `jsonld.preloaded_contexts.url`.
    #[display("jsonld.preloaded_contexts.url")]
    JsonLdPreloadedContextUrl,
    /// `entity.spatial_index.rebuild_interval`.
    #[display("entity.spatial_index.rebuild_interval")]
    EntitySpatialIndexRebuildInterval,
    /// `entity.side_effects.retention.dead_letter_after`.
    #[display("entity.side_effects.retention.dead_letter_after")]
    EntitySideEffectsDeadLetterAfter,
    /// `entity.side_effects.retention.sweep_interval`.
    #[display("entity.side_effects.retention.sweep_interval")]
    EntitySideEffectsRetentionSweepInterval,
    /// A key of one durable queue table.
    #[display("{_0}.{_1}")]
    Queue(QueueName, QueueKey),
    /// `temporal.query.default_last_n`.
    #[display("temporal.query.default_last_n")]
    TemporalQueryDefaultLastN,
    /// `temporal.query.max_last_n`.
    #[display("temporal.query.max_last_n")]
    TemporalQueryMaxLastN,
    /// `temporal.query.max_edge_group`.
    #[display("temporal.query.max_edge_group")]
    TemporalQueryMaxEdgeGroup,
    /// `temporal.query.max_aggregation_instances`.
    #[display("temporal.query.max_aggregation_instances")]
    TemporalQueryMaxAggregationInstances,
    /// `temporal.storage.chunk_interval`.
    #[display("temporal.storage.chunk_interval")]
    TemporalStorageChunkInterval,
    /// `temporal.storage.columnstore.convert_after`.
    #[display("temporal.storage.columnstore.convert_after")]
    TemporalStorageConvertAfter,
    /// `temporal.storage.columnstore.sweep_interval`.
    #[display("temporal.storage.columnstore.sweep_interval")]
    TemporalStorageColumnstoreSweepInterval,
    /// `temporal.storage.retention.delete_after`.
    #[display("temporal.storage.retention.delete_after")]
    TemporalStorageDeleteAfter,
    /// `temporal.storage.retention.sweep_interval`.
    #[display("temporal.storage.retention.sweep_interval")]
    TemporalStorageRetentionSweepInterval,
    /// `temporal.storage.retention.mode`.
    #[display("temporal.storage.retention.mode")]
    TemporalStorageRetentionMode,
    /// `notifications.delivery.request_timeout`.
    #[display("notifications.delivery.request_timeout")]
    NotificationsDeliveryRequestTimeout,
    /// `notifications.evaluation.shutdown_drain`.
    #[display("notifications.evaluation.shutdown_drain")]
    NotificationsEvaluationShutdownDrain,
    /// `notifications.delivery.max_http_body`.
    #[display("notifications.delivery.max_http_body")]
    NotificationsDeliveryMaxHttpBody,
    /// `notifications.delivery.max_attempts`.
    #[display("notifications.delivery.max_attempts")]
    NotificationsDeliveryMaxAttempts,
    /// `notifications.delivery.max_mqtt_packet`.
    #[display("notifications.delivery.max_mqtt_packet")]
    NotificationsDeliveryMaxMqttPacket,
    /// `notifications.delivery.max_concurrent`.
    #[display("notifications.delivery.max_concurrent")]
    NotificationsDeliveryMaxConcurrent,
    /// `notifications.delivery.max_error_length`.
    #[display("notifications.delivery.max_error_length")]
    NotificationsDeliveryMaxErrorLength,
    /// `notifications.delivery.retention.acknowledged_after`.
    #[display("notifications.delivery.retention.acknowledged_after")]
    NotificationsDeliveryAcknowledgedAfter,
    /// `notifications.delivery.retention.dead_letter_after`.
    #[display("notifications.delivery.retention.dead_letter_after")]
    NotificationsDeliveryDeadLetterAfter,
    /// `notifications.delivery.retention.orphan_event_after`.
    #[display("notifications.delivery.retention.orphan_event_after")]
    NotificationsDeliveryOrphanEventAfter,
    /// `notifications.delivery.retention.sweep_interval`.
    #[display("notifications.delivery.retention.sweep_interval")]
    NotificationsDeliveryRetentionSweepInterval,
    /// `notifications.periodic.max_poll_interval`.
    #[display("notifications.periodic.max_poll_interval")]
    NotificationsPeriodicMaxPollInterval,
    /// `federation.forwarding.connect_timeout`.
    #[display("federation.forwarding.connect_timeout")]
    FederationForwardingConnectTimeout,
    /// `federation.forwarding.request_timeout`.
    #[display("federation.forwarding.request_timeout")]
    FederationForwardingRequestTimeout,
    /// `federation.query.max_entities`.
    #[display("federation.query.max_entities")]
    FederationQueryMaxEntities,
    /// `federation.query.max_source_rows`.
    #[display("federation.query.max_source_rows")]
    FederationQueryMaxSourceRows,
    /// `federation.cooldown.mode`.
    #[display("federation.cooldown.mode")]
    FederationCooldownMode,
    /// `federation.cooldown.retry_interval`.
    #[display("federation.cooldown.retry_interval")]
    FederationCooldownRetryInterval,
    /// `federation.cooldown.max_retry_interval`.
    #[display("federation.cooldown.max_retry_interval")]
    FederationCooldownMaxRetryInterval,
    /// `snapshot.default_lifetime`.
    #[display("snapshot.default_lifetime")]
    SnapshotDefaultLifetime,
    /// `snapshot.max_lifetime`.
    #[display("snapshot.max_lifetime")]
    SnapshotMaxLifetime,
    /// `snapshot.quota.max_storage_per_tenant`.
    #[display("snapshot.quota.max_storage_per_tenant")]
    SnapshotMaxStoragePerTenant,
    /// `snapshot.preparation.lease_timeout`.
    #[display("snapshot.preparation.lease_timeout")]
    SnapshotPreparationLeaseTimeout,
    /// `snapshot.preparation.recovery_interval`.
    #[display("snapshot.preparation.recovery_interval")]
    SnapshotPreparationRecoveryInterval,
    /// `snapshot.preparation.scoped_operation_timeout`.
    #[display("snapshot.preparation.scoped_operation_timeout")]
    SnapshotPreparationScopedOperationTimeout,
    /// `observability.metrics.prometheus.mode`.
    #[display("observability.metrics.prometheus.mode")]
    ObservabilityMetricsPrometheusMode,
}

#[cfg(test)]
mod tests {
    use crate::validate::config_key::{ConfigKey, QueueKey, QueueName};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_queue_key_is_its_table_path_and_its_key() {
        assert_eq!(
            ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::MaxRetryInterval).to_string(),
            "notifications.delivery.queue.max_retry_interval"
        );
    }

    #[test]
    fn a_table_key_is_its_dotted_path() {
        assert_eq!(ConfigKey::TemporalStorageConvertAfter.to_string(), "temporal.storage.columnstore.convert_after");
    }
}
