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
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_subscription::{csource::CsrSubscriptionVariant, lifecycle::SubscriptionService, store::SubscriptionStore};
use andromeda_tenancy::tenant_ref::TenantRef;
use std::sync::Arc;
use typed_builder::TypedBuilder;
use urn_rs::UrnBuilder;
use uuid::Uuid;

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
    /// JSON-LD processing for the filters applied at re-assembly.
    pub(crate) engine: JsonLdEngine,
}

impl SubscriptionForwardingService {
    /// Mint an identifier for a subscription this broker hands to a peer: a
    /// companion or a forwarded copy. Clause 5.2.12 lets the creator choose
    /// the identifier, so the broker chooses one it can route back.
    pub(crate) fn mint_subscription_id(&self) -> Result<Uri, urn_rs::Error> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:forwarded:{}", Uuid::new_v4())).build()?;
        Ok(Uri::from(urn))
    }
}
