use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
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
    /// The tenant's stored `@context`s, and the host of the contexts its
    /// responses and subscriptions name.
    pub context_hosting: TenantContextHosting,
    pub csr: Arc<RegistrationService>,
    pub csr_subscriptions: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    pub forwarding: Arc<FederationService>,
    pub entity_maps: Arc<EntityMapService>,
    /// The distributed side of subscriptions; absent when no advertised URL
    /// makes this broker reachable for the notifications it would receive.
    pub subscription_forwarding: Option<Arc<SubscriptionForwardingService>>,
    pub snapshots: Arc<SnapshotService>,
}
