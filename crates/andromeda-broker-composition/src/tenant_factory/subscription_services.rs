//! Construction of one tenant's subscription services, local and distributed.

use crate::{
    event::{
        subscription_change_sink::{BroadcastSubscriptionChangeSink, CoherentSubscriptionChangeSink},
        subscription_endpoint_validator::DeliverySubscriptionEndpointValidator,
    },
    tenant_factory::{storage::TenantStorage, tasks::TenantSignals, transport::TenantTransports},
};
use andromeda_context_registry::service::ContextRegistryService;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::FederationService;
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_postgresql_context_store::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    subscription::repo::SubscriptionRepository,
    subscription_forwarding::repo::SubscriptionForwardingRepository,
};
use andromeda_subscription::{
    csource::CsrSubscriptionVariant,
    endpoint_admission::SubscriptionEndpointValidator,
    entity::EntitySubscriptionVariant,
    forwarding_hook::SubscriptionForwardingHook,
    lifecycle::SubscriptionService,
    store::SubscriptionStore,
};
use andromeda_subscription_forwarding::{
    mapping::{ForwardedSubscriptionRouter, SubscriptionForwardingStore},
    receiver::ReceiverEndpoint,
    service::SubscriptionForwardingService,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use std::sync::Arc;

/// The subscription services one tenant answers requests with.
pub(super) struct SubscriptionServices {
    /// Entity subscriptions (clause 5.8).
    pub(super) entity: Arc<SubscriptionService<EntitySubscriptionVariant>>,
    /// Context Source Registration subscriptions (clause 5.11).
    pub(super) csource: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    /// The distributed side of subscriptions, absent when nothing advertises
    /// an origin peers could deliver notifications back to.
    pub(super) forwarding: Option<Arc<SubscriptionForwardingService>>,
}

/// The distributed edge one tenant's subscription forwarding is defined over.
///
/// The three belong together because each is required for the same thing to
/// exist: clauses 5.8.1.4 and 5.8.6 need an origin peers can reach, a
/// notification arriving there is routed back by the minted identifier, and the
/// subscriptions it was forwarded to are the registrations federation resolved.
pub(super) struct DistributedSubscriptionEdge<'edge> {
    /// The origin below which peers deliver notifications back to this broker.
    receiver: ReceiverEndpoint,
    /// The platform route from a minted subscription identifier to its tenant.
    router: &'edge Arc<dyn ForwardedSubscriptionRouter>,
    /// The forwarding service registered context sources are reached through.
    federation: &'edge Arc<FederationService>,
}

impl<'edge> DistributedSubscriptionEdge<'edge> {
    /// Resolve the edge from the advertised origin.
    ///
    /// Without an advertised origin the distributed side stays off, and the
    /// broker says so once per tenant.
    pub(super) fn resolve(
        info: &InfoConfig,
        router: &'edge Arc<dyn ForwardedSubscriptionRouter>,
        federation: &'edge Arc<FederationService>,
        tenant: &TenantRef,
    ) -> Option<DistributedSubscriptionEdge<'edge>> {
        let Some(receiver) = info.advertised_url.as_ref().and_then(ReceiverEndpoint::from_advertised) else {
            tracing::info!(tenant = ?tenant, "subscription forwarding disabled: ANDROMEDA_INFO_ADVERTISED_URL is not set");
            return None;
        };
        Some(DistributedSubscriptionEdge { receiver, router, federation })
    }
}

/// Compose both subscription services and the distributed side joining them.
pub(super) fn compose_subscription_services(
    storage: &TenantStorage,
    signals: &TenantSignals,
    transports: &TenantTransports,
    edge: Option<DistributedSubscriptionEdge<'_>>,
    entity_query: &Arc<EntityQueryService>,
    contexts: &Arc<ContextRegistryService>,
) -> SubscriptionServices {
    let endpoint_validator: Arc<dyn SubscriptionEndpointValidator> = Arc::new(DeliverySubscriptionEndpointValidator);
    let csource = Arc::new(SubscriptionService::new(
        // clone: the subscription stores intern attribute lists through the tenant's one dictionary.
        Arc::new(CsourceSubscriptionRepository::new(&storage.pool, Arc::clone(&storage.attribute_names))),
        storage.engine.clone(),                              // clone: each subscription service owns its engine handle.
        CsrSubscriptionVariant::new(storage.engine.clone()), // clone: the variant expands against its own engine handle.
        Some(Arc::new(BroadcastSubscriptionChangeSink::new(signals.csource_subscription.clone()))), // clone: service and worker each own a sender handle.
        Arc::clone(&endpoint_validator),                     // clone: both subscription services share one endpoint validator.
    ));
    // clone: see above.
    let store = Arc::new(SubscriptionRepository::new(&storage.pool, Arc::clone(&storage.attribute_names))) as Arc<dyn SubscriptionStore>;
    let forwarding = edge.map(|edge| compose_subscription_forwarding(edge, storage, transports, entity_query, &csource, &store));
    let mut entity = SubscriptionService::new(
        Arc::clone(&store),
        storage.engine.clone(), // clone: each subscription service owns its engine handle.
        EntitySubscriptionVariant::new(storage.engine.clone(), Arc::clone(contexts)), // clone: the variant expands against its own engine handle.
        Some(Arc::new(CoherentSubscriptionChangeSink::new(
            signals.subscription.clone(), // clone: service and worker each own a sender handle.
            Arc::clone(&storage.subscription_cache),
            store,
        ))),
        endpoint_validator,
    );
    if let Some(forwarding_service) = &forwarding {
        entity = entity.with_forwarding_hook(Arc::clone(forwarding_service) as Arc<dyn SubscriptionForwardingHook>);
    }
    SubscriptionServices {
        entity: Arc::new(entity),
        csource,
        forwarding,
    }
}

/// Compose the distributed side of subscriptions for one tenant.
fn compose_subscription_forwarding(
    edge: DistributedSubscriptionEdge<'_>,
    storage: &TenantStorage,
    transports: &TenantTransports,
    entity_query: &Arc<EntityQueryService>,
    csource_subscriptions: &Arc<SubscriptionService<CsrSubscriptionVariant>>,
    subscriptions: &Arc<dyn SubscriptionStore>,
) -> Arc<SubscriptionForwardingService> {
    Arc::new(
        SubscriptionForwardingService::builder()
            .live_target(storage.live_target.clone()) // clone: forwarding is defined only over the tenant's live branch.
            .receiver(edge.receiver)
            .tenant(storage.tenant.clone()) // clone: the forwarding service owns its tenant
            .subscriptions(Arc::clone(subscriptions))
            .csource_subscriptions(Arc::clone(csource_subscriptions))
            .mappings(Arc::new(SubscriptionForwardingRepository::new(&storage.pool)) as Arc<dyn SubscriptionForwardingStore>)
            .router(Arc::clone(edge.router))
            .federation(Arc::clone(edge.federation))
            .entity_query(Arc::clone(entity_query))
            .transport(Arc::clone(&transports.dispatcher))
            .engine(storage.engine.clone()) // clone: forwarding owns its engine handle.
            .build(),
    )
}
