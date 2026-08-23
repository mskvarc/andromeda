//! Focused state resolution for entity route handlers.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    context_target::{ContextTargetProvider, ExistingTargetedTenantState, ProvisionableTargetedTenantState, SnapshotTargetResolver},
    tenant_state::TenantStateResolver,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// The complete, entity-only capability set one tenant exposes to the entity binding.
#[derive(TypedBuilder)]
pub struct EntityRouteServices {
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
    /// Tenant's ordinary live context branch.
    pub live_target: ContextTarget,
    /// Snapshot URI to durable branch resolution.
    pub snapshot_targets: Arc<dyn SnapshotTargetResolver>,
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

/// Resolves the focused entity capabilities for one typed tenant scope.
pub trait EntityStateResolver: Send + Sync {
    /// Resolve or create the tenant according to `creation_policy`, returning no unrelated tenant capabilities.
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<EntityRouteServices>, TenancyError>>;
}

/// Axum state held only by the state-complete entity route fragment.
#[derive(Clone)]
pub(crate) struct EntityRouteState {
    /// Focused per-tenant capability resolver.
    resolver: Arc<dyn EntityStateResolver>,
}

impl EntityRouteState {
    /// Construct route state around the composition-supplied resolver.
    pub(crate) const fn new(resolver: Arc<dyn EntityStateResolver>) -> EntityRouteState {
        EntityRouteState { resolver }
    }
}

impl TenantStateResolver<Arc<EntityRouteServices>> for EntityRouteState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<Arc<EntityRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
