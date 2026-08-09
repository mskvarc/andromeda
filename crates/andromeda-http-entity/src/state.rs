//! Focused state resolution for entity route handlers.

use crate::{error::classify_tenancy_error, snapshot_scope::EntitySnapshotScope};
use andromeda_entity::{EntityService, discovery::EntityDiscoveryService, query::EntityQueryService};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_extraction::tenant_state::{ExistingTenantState, ProvisionableTenantState, TenantStateResolver};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
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
    /// Frozen entity reads for snapshot-scoped requests.
    pub snapshots: Arc<dyn EntitySnapshotScope>,
}

/// Existing-tenant extractor specialized to the entity binding's focused state.
pub(crate) type EntityTenantState = ExistingTenantState<Arc<EntityRouteServices>>;

/// Provisionable-tenant extractor specialized to the entity binding's focused state.
pub(crate) type EntityTenantStateOrCreate = ProvisionableTenantState<Arc<EntityRouteServices>>;

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
        instance: &'a axum::http::Uri,
    ) -> BoxFuture<'a, Result<Arc<EntityRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
