use crate::{
    service::{
        csource_forwarding::CsourceForwardingService,
        csource_registration::CsourceRegistrationService,
        csource_subscription::CsourceSubscriptionService,
        entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService},
        entity_map::EntityMapService,
        jsonld_context::JsonLdContextService,
        snapshot::SnapshotService,
        subscription::SubscriptionService,
        temporal::TemporalService,
    },
    transport::{sse::SseConnectionRegistry, ws::WsConnectionRegistry},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AndromedaState {
    pub entities: Arc<EntityService>,
    pub entity_query: Arc<EntityQueryService>,
    pub entity_discovery: Arc<EntityDiscoveryService>,
    pub temporal: Arc<TemporalService>,
    pub subscriptions: Arc<SubscriptionService>,
    pub contexts: Arc<JsonLdContextService>,
    pub csr: Arc<CsourceRegistrationService>,
    pub csr_subscriptions: Arc<CsourceSubscriptionService>,
    pub forwarding: Arc<CsourceForwardingService>,
    pub entity_maps: Arc<EntityMapService>,
    pub snapshots: Arc<SnapshotService>,
    pub sse_registry: Arc<SseConnectionRegistry>,
    /// Per-connection SSE channel buffer size.
    pub sse_channel_buffer_size: usize,
    /// SSE keep-alive ping interval in seconds.
    pub sse_keep_alive_interval_secs: u64,
    pub ws_registry: Arc<WsConnectionRegistry>,
    /// Per-connection WebSocket channel buffer size.
    pub ws_channel_buffer_size: usize,
    /// WebSocket ping interval in seconds.
    pub ws_ping_interval_secs: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub tenants: Arc<crate::tenancy::runtime::TenantRuntimeManager>,
    pub source_identity: String,
}
