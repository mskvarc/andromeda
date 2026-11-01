//! The service tying the distributed subscription side together.

use crate::{
    mapping::{ForwardedSubscriptionRouter, SubscriptionForwardingStore},
    receiver::ReceiverEndpoint,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::FederationService;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::{csource::CsrSubscriptionVariant, lifecycle::SubscriptionService, store::SubscriptionStore};
use andromeda_tenancy::tenant_ref::TenantRef;
use lockable::LockPool;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// The capabilities one tenant's distributed subscription side is built from.
#[derive(TypedBuilder)]
pub struct SubscriptionForwardingService {
    /// Live context branch used by this federation-only service.
    pub(crate) live_target: ContextTarget,
    /// The endpoint context sources notify this broker at.
    pub(crate) receiver: ReceiverEndpoint,
    /// The tenant the service belongs to.
    pub(crate) tenant: TenantRef,
    /// The entity subscriptions being mirrored.
    pub(crate) subscriptions: Arc<dyn SubscriptionStore>,
    /// The context source registration subscriptions the companions live in.
    pub(crate) csource_subscriptions: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    /// The tenant-scoped identifier mappings.
    pub(crate) mappings: Arc<dyn SubscriptionForwardingStore>,
    /// The platform-wide route from minted identifiers to tenants.
    pub(crate) router: Arc<dyn ForwardedSubscriptionRouter>,
    /// Forwarding to context sources.
    pub(crate) federation: Arc<FederationService>,
    /// Local entity reads for re-assembly.
    pub(crate) entity_query: Arc<EntityQueryService>,
    /// Delivery to the local subscriber's endpoint.
    pub(crate) transport: Arc<TransportDispatcher>,
    /// JSON-LD processing for the entities re-assembled.
    pub(crate) engine: JsonLdEngine,
    /// One lock per local subscription whose lifecycle step is in flight.
    ///
    /// The steps of one subscription (creation, update, expiry, deletion) read
    /// the companion and copy mappings and then act on them at the sources, so
    /// two of them running at once act on what the other is changing. A pool
    /// entry exists only while a step holds or awaits it. One broker serves a
    /// tenant database, so the lock is per process.
    #[builder(default = LockPool::new(), setter(skip))]
    pub(crate) lifecycle_steps: LockPool<Identifier>,
}
