use crate::service::{
    context_source_forwarding::ContextSourceForwardingService,
    context_source_registration::ContextSourceRegistrationService,
    csource_subscription::CsourceSubscriptionService,
    entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService},
    entity_map::EntityMapService,
    jsonld_context::JsonLdContextService,
    snapshot::SnapshotService,
    subscription::SubscriptionService,
    temporal::TemporalService,
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
    pub csr: Arc<ContextSourceRegistrationService>,
    pub csr_subscriptions: Arc<CsourceSubscriptionService>,
    pub forwarding: Arc<ContextSourceForwardingService>,
    pub entity_maps: Arc<EntityMapService>,
    pub snapshots: Arc<SnapshotService>,
}

#[derive(Clone)]
pub struct AppState {
    pub tenants: Arc<crate::tenancy::TenantRuntimeManager>,
    pub source_identity: String,
}
