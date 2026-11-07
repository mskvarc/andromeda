//! What a rule reads of a key that holds no quantity: the keyword it is
//! written as, whether it is set, or the entries of its list.

use crate::{
    broker_config::BrokerConfig,
    schema::{
        http::cors::{ExposeRule, HeaderRule, MethodRule, OriginRule},
        jsonld::PreloadedContext,
    },
    validate::{choice_value::ChoiceValue, config_key::ConfigKey},
};

/// What a key holds, as a compound rule reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Holding<'config> {
    /// The key is written as this keyword.
    Choice(ChoiceValue),
    /// Whether the optional key is set, or the list holds an entry.
    Presence(Presence),
    /// The entries of `jsonld.preloaded_contexts`.
    PreloadedContexts(&'config [PreloadedContext]),
}

impl<'config> Holding<'config> {
    /// The keyword held; `None` for a key that holds none.
    pub(crate) const fn choice(self) -> Option<ChoiceValue> {
        match self {
            Holding::Choice(choice) => Some(choice),
            Holding::Presence(_) | Holding::PreloadedContexts(_) => None,
        }
    }

    /// Whether the key is set; `None` for a key that is never left out.
    pub(crate) const fn presence(self) -> Option<Presence> {
        match self {
            Holding::Presence(presence) => Some(presence),
            Holding::Choice(_) | Holding::PreloadedContexts(_) => None,
        }
    }

    /// The preloaded contexts; `None` for any other key.
    pub(crate) const fn preloaded_contexts(self) -> Option<&'config [PreloadedContext]> {
        match self {
            Holding::PreloadedContexts(contexts) => Some(contexts),
            Holding::Choice(_) | Holding::Presence(_) => None,
        }
    }
}

/// Whether an optional key is set, or a list holds an entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Presence {
    /// The key is left out, or the list is empty.
    Absent,
    /// The key is set, or the list holds an entry.
    Present,
}

impl ConfigKey {
    /// What the key holds in `config` as a compound rule reads it.
    ///
    /// `None` for a key that holds a quantity, which [`ConfigKey::reading`]
    /// reads instead, and for a CORS list written as its entries, which holds
    /// no keyword.
    pub(crate) fn holding(self, config: &BrokerConfig) -> Option<Holding<'_>> {
        let http = &config.http;
        let tls = &config.tls;
        match self {
            ConfigKey::ServerTransport => Some(choice(config.server.transport)),
            ConfigKey::ServerHttp3Mode => Some(choice(config.server.http3.mode)),
            ConfigKey::ManagementMode => Some(choice(config.management.mode)),
            ConfigKey::ManagementTransport => Some(choice(config.management.transport)),
            ConfigKey::TlsCertificate => Some(optional(tls.certificate.as_ref())),
            ConfigKey::TlsPrivateKey => Some(optional(tls.private_key.as_ref())),
            ConfigKey::TlsClientAuthenticationCa => Some(optional(tls.client_authentication.ca.as_ref())),
            ConfigKey::TlsClientAuthenticationMode => Some(choice(tls.client_authentication.mode)),
            ConfigKey::HttpCorsMode => Some(choice(http.cors.mode)),
            ConfigKey::HttpCorsCredentials => Some(choice(http.cors.credentials)),
            ConfigKey::HttpCorsAllowOrigins => matches!(http.cors.allow_origins, OriginRule::Any).then_some(ANY),
            ConfigKey::HttpCorsAllowMethods => matches!(http.cors.allow_methods, MethodRule::Any).then_some(ANY),
            ConfigKey::HttpCorsAllowHeaders => matches!(http.cors.allow_headers, HeaderRule::Any).then_some(ANY),
            ConfigKey::HttpCorsExposeHeaders => matches!(http.cors.expose_headers, ExposeRule::Any).then_some(ANY),
            ConfigKey::HttpCompressionMode => Some(choice(http.compression.mode)),
            ConfigKey::HttpCompressionAlgorithms => Some(listed(http.compression.algorithms.iter())),
            ConfigKey::HttpDecompressionMode => Some(choice(http.decompression.mode)),
            ConfigKey::HttpDecompressionAlgorithms => Some(listed(http.decompression.algorithms.iter())),
            ConfigKey::HttpForwardedTrust => Some(choice(http.forwarded.trust)),
            ConfigKey::HttpForwardedProxies => Some(listed(http.forwarded.proxies.iter())),
            ConfigKey::JsonLdPreloadedContextUrl => Some(Holding::PreloadedContexts(&config.jsonld.preloaded_contexts)),
            ConfigKey::TemporalStorageDeleteAfter => Some(optional(config.temporal.storage.retention.delete_after.as_ref())),
            ConfigKey::TemporalStorageRetentionMode => Some(choice(config.temporal.storage.retention.mode)),
            ConfigKey::FederationCooldownMode => Some(choice(config.federation.cooldown.mode)),
            ConfigKey::ObservabilityMetricsPrometheusMode => Some(choice(config.observability.metrics.prometheus.mode)),
            ConfigKey::ServerPort
            | ConfigKey::ServerGracefulDrainTimeout
            | ConfigKey::ServerAnnouncedCloseTimeout
            | ConfigKey::ManagementPort
            | ConfigKey::TlsHandshakeTimeout
            | ConfigKey::RequestLimitsTimeout
            | ConfigKey::RequestLimitsMaxBody
            | ConfigKey::RequestLimitsMaxPageBody
            | ConfigKey::RequestLimitsMaxHeaderSection
            | ConfigKey::RequestLimitsMaxAttributeNesting
            | ConfigKey::RequestLimitsMaxQNesting
            | ConfigKey::HttpBodyDecodingBlockingThreshold
            | ConfigKey::DatabasePoolerMaxConnectionsPerTenant
            | ConfigKey::DatabaseWriteReservePercent
            | ConfigKey::DatabaseAcquireTimeout
            | ConfigKey::DatabaseNameCacheMaxSizePerDictionary
            | ConfigKey::DatabaseNameCacheIdleEvictionAfter
            | ConfigKey::DatabaseMaintenanceMaxRepackSize
            | ConfigKey::TenancyProvisionalTenantLifetime
            | ConfigKey::TenancyDatabasesMaxConnections
            | ConfigKey::TenancyResidencyIdleEvictionAfter
            | ConfigKey::TenancyResidencySweepInterval
            | ConfigKey::TenancyResidencyMinWakeInterval
            | ConfigKey::TenancyResidencyWokenLifetime
            | ConfigKey::TenancyBuildAdmissionTimeout
            | ConfigKey::ExpirySweepInterval
            | ConfigKey::JsonLdFetchTimeout
            | ConfigKey::JsonLdFetchMaxSize
            | ConfigKey::JsonLdCacheHitFlushInterval
            | ConfigKey::JsonLdCacheDefaultLifetime
            | ConfigKey::JsonLdCacheHostedRevalidationInterval
            | ConfigKey::EntitySpatialIndexRebuildInterval
            | ConfigKey::EntitySideEffectsDeadLetterAfter
            | ConfigKey::EntitySideEffectsRetentionSweepInterval
            | ConfigKey::Queue(..)
            | ConfigKey::TemporalQueryDefaultLastN
            | ConfigKey::TemporalQueryMaxLastN
            | ConfigKey::TemporalQueryMaxEdgeGroup
            | ConfigKey::TemporalQueryMaxAggregationInstances
            | ConfigKey::TemporalStorageChunkInterval
            | ConfigKey::TemporalStorageConvertAfter
            | ConfigKey::TemporalStorageColumnstoreSweepInterval
            | ConfigKey::TemporalStorageRetentionSweepInterval
            | ConfigKey::NotificationsDeliveryRequestTimeout
            | ConfigKey::NotificationsEvaluationDrainTimeout
            | ConfigKey::NotificationsDeliveryMaxHttpBody
            | ConfigKey::NotificationsDeliveryMaxAttempts
            | ConfigKey::NotificationsDeliveryMaxMqttPacket
            | ConfigKey::NotificationsDeliveryMaxConcurrent
            | ConfigKey::NotificationsDeliveryMaxErrorLength
            | ConfigKey::NotificationsDeliveryAcknowledgedAfter
            | ConfigKey::NotificationsDeliveryDeadLetterAfter
            | ConfigKey::NotificationsDeliveryOrphanEventAfter
            | ConfigKey::NotificationsDeliveryRetentionSweepInterval
            | ConfigKey::NotificationsPeriodicMaxPollInterval
            | ConfigKey::FederationForwardingConnectTimeout
            | ConfigKey::FederationForwardingRequestTimeout
            | ConfigKey::FederationQueryMaxEntities
            | ConfigKey::FederationQueryMaxSourceRows
            | ConfigKey::FederationCooldownRetryInterval
            | ConfigKey::FederationCooldownMaxRetryInterval
            | ConfigKey::SnapshotDefaultLifetime
            | ConfigKey::SnapshotMaxLifetime
            | ConfigKey::SnapshotMaxStoragePerTenant
            | ConfigKey::SnapshotPreparationLeaseTimeout
            | ConfigKey::SnapshotPreparationRecoveryInterval
            | ConfigKey::SnapshotPreparationScopedOperationTimeout => None,
        }
    }
}

/// A CORS list written as `"any"`.
const ANY: Holding<'static> = Holding::Choice(ChoiceValue::Any);

/// A keyword as a holding.
fn choice<'config>(keyword: impl Into<ChoiceValue>) -> Holding<'config> {
    Holding::Choice(keyword.into())
}

/// An optional key's value as a holding.
const fn optional<'config, T>(value: Option<&T>) -> Holding<'config> {
    Holding::Presence(match value {
        Some(_) => Presence::Present,
        None => Presence::Absent,
    })
}

/// A list's entries as a holding.
fn listed<'config>(mut entries: impl Iterator) -> Holding<'config> {
    Holding::Presence(match entries.next() {
        Some(_) => Presence::Present,
        None => Presence::Absent,
    })
}
