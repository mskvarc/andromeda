use andromeda_context_registry::service::ContextRegistryService;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage};
use andromeda_entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_registration::RegistrationService;
use andromeda_snapshot::service::SnapshotService;
use andromeda_subscription::{csource::CsrSubscriptionVariant, entity::EntitySubscriptionVariant, lifecycle::SubscriptionService};
use andromeda_subscription_forwarding::service::SubscriptionForwardingService;
use andromeda_temporal::service::TemporalService;
use std::sync::Arc;

#[derive(Clone)]
pub struct AndromedaState {
    /// The tenant's durable live context-space target.
    pub live_target: ContextTarget,
    pub entities: Arc<EntityService>,
    pub entity_query: Arc<EntityQueryService>,
    pub entity_discovery: Arc<EntityDiscoveryService>,
    pub temporal: Arc<TemporalService>,
    pub subscriptions: Arc<SubscriptionService<EntitySubscriptionVariant>>,
    pub contexts: Arc<ContextRegistryService>,
    pub csr: Arc<RegistrationService>,
    pub csr_subscriptions: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    pub forwarding: Arc<FederationService>,
    pub entity_maps: Arc<EntityMapService>,
    /// The distributed side of subscriptions; absent when no advertised URL
    /// makes this broker reachable for the notifications it would receive.
    pub subscription_forwarding: Option<Arc<SubscriptionForwardingService>>,
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
