//! The request-facing state one tenant's routes share: its live context-space
//! target and every service a handler reaches.

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

/// One tenant's request-facing services, built once per tenant and cloned into
/// each router; every service is behind an `Arc`, so a clone shares them.
#[derive(Clone)]
pub(crate) struct AndromedaState {
    /// The tenant's durable live context-space target.
    pub live_target: ContextTarget,
    /// Entity create, update, merge, replace and delete, single and batch.
    pub entities: Arc<EntityService>,
    /// Entity retrieval and query, local and forwarded to registered sources.
    pub entity_query: Arc<EntityQueryService>,
    /// Type and attribute discovery over local and registered entities.
    pub entity_discovery: Arc<EntityDiscoveryService>,
    /// Temporal entity reads and history mutations.
    pub temporal: Arc<TemporalService>,
    /// Lifecycle of entity Subscriptions.
    pub subscriptions: Arc<SubscriptionService<EntitySubscriptionVariant>>,
    /// The tenant's stored `@context`s, and the host of the contexts its
    /// responses and subscriptions name.
    pub context_hosting: TenantContextHosting,
    /// Lifecycle of Context Source Registrations.
    pub csr: Arc<RegistrationService>,
    /// Lifecycle of Context Source Registration Subscriptions.
    pub csr_subscriptions: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    /// Forwarding of requests to the registered context sources that match them.
    pub forwarding: Arc<FederationService>,
    /// The Entity Maps a distributed query records and later pages read back.
    pub entity_maps: Arc<EntityMapService>,
    /// The distributed side of subscriptions; absent when no advertised URL
    /// makes this broker reachable for the notifications it would receive.
    pub subscription_forwarding: Option<Arc<SubscriptionForwardingService>>,
    /// Snapshot create, retrieve, update, delete and clone.
    pub snapshots: Arc<SnapshotService>,
}
