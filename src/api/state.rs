use crate::{
    api::tenant_factory::BrokerTenantFactory,
    service::{
        csource_forwarding::CsourceForwardingService,
        csource_registration::CsourceRegistrationService,
        csource_subscription::CsrSubscriptionVariant,
        entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService},
        entity_map::EntityMapService,
        jsonld_context::JsonLdContextService,
        snapshot::SnapshotService,
        subscription::EntitySubscriptionVariant,
        subscription_base::SubscriptionServiceCore,
        temporal::TemporalService,
    },
};
use andromeda_delivery::stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use andromeda_postgresql_context_store::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    subscription::repo::SubscriptionRepository,
    tenant_registry::PostgresTenantRegistry,
};
use andromeda_tenancy::runtime::TenantRuntimeManager;
use std::sync::Arc;

#[derive(Clone)]
pub struct AndromedaState {
    pub entities: Arc<EntityService>,
    pub entity_query: Arc<EntityQueryService>,
    pub entity_discovery: Arc<EntityDiscoveryService>,
    pub temporal: Arc<TemporalService>,
    pub subscriptions: Arc<SubscriptionServiceCore<SubscriptionRepository, EntitySubscriptionVariant>>,
    pub contexts: Arc<JsonLdContextService>,
    pub csr: Arc<CsourceRegistrationService>,
    pub csr_subscriptions: Arc<SubscriptionServiceCore<CsourceSubscriptionRepository, CsrSubscriptionVariant>>,
    pub forwarding: Arc<CsourceForwardingService>,
    pub entity_maps: Arc<EntityMapService>,
    pub snapshots: Arc<SnapshotService>,
    pub sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
    /// Per-connection SSE channel buffer size.
    pub sse_channel_buffer_size: usize,
    /// SSE keep-alive ping interval in seconds.
    pub sse_keep_alive_interval_secs: u64,
    pub ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
    /// Per-connection WebSocket channel buffer size.
    pub ws_channel_buffer_size: usize,
    /// WebSocket ping interval in seconds.
    pub ws_ping_interval_secs: u64,
}

#[derive(Clone)]
pub struct BrokerState {
    pub tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
    pub source_id: Uri,
    pub source_alias: ContextSourcePseudonym,
}
