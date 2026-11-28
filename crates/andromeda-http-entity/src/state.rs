//! Focused state resolution for entity route handlers.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::context_target::{ContextTargetProvider, ExistingTargetedTenantState, ProvisionableTargetedTenantState, SnapshotTargetResolver};
use andromeda_posted_query::service::PostedQueryService;
use andromeda_tenancy::runtime::lease::RuntimeLease;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// The complete, entity-only capability set one tenant exposes to the entity binding.
#[derive(TypedBuilder)]
pub struct EntityRouteServices {
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
    /// Entity mutation policy.
    pub entities: Arc<EntityService>,
    /// Entity query policy.
    pub entity_query: Arc<EntityQueryService>,
    /// Type and attribute discovery policy.
    pub entity_discovery: Arc<EntityDiscoveryService>,
    /// Distributed entity forwarding policy.
    pub forwarding: Arc<FederationService>,
    /// Entity Map policy used by entity query scoping and creation.
    pub entity_maps: Arc<EntityMapService>,
    /// The paged queries sent by POST the tenant holds for the page links of
    /// their result.
    pub posted_queries: Arc<PostedQueryService>,
    /// Tenant's ordinary live context branch.
    pub live_target: ContextTarget,
    /// Snapshot URI to durable branch resolution.
    pub snapshot_targets: Arc<dyn SnapshotTargetResolver>,
    /// The tenant's host of the contexts its responses name when no single
    /// URL names them (Annex C.8).
    pub context_hosting: TenantContextHosting,
}

/// Existing-tenant extractor specialized to the entity binding's focused state.
pub(crate) type EntityTenantState = ExistingTargetedTenantState<Arc<EntityRouteServices>>;

/// Provisionable-tenant extractor specialized to the entity binding's focused state.
pub(crate) type EntityTenantStateOrCreate = ProvisionableTargetedTenantState<Arc<EntityRouteServices>>;

impl ContextTargetProvider for EntityRouteServices {
    fn live_target(&self) -> &ContextTarget {
        &self.live_target
    }

    fn snapshot_target_resolver(&self) -> &dyn SnapshotTargetResolver {
        self.snapshot_targets.as_ref()
    }
}
