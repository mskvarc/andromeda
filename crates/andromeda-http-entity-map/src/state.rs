//! Focused tenant-state resolution for Entity Map routes.

use andromeda_entity::query::EntityQueryService;
use andromeda_entity_map::service::EntityMapService;
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    tenant_state::{ExistingTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;

/// The complete Entity Map capability set exposed by one tenant.
pub struct EntityMapRouteServices {
    /// Entity Map lifecycle policy.
    pub entity_maps: Arc<EntityMapService>,
    /// Entity query policy used to materialize map membership.
    pub entity_query: Arc<EntityQueryService>,
}

impl EntityMapRouteServices {
    /// Construct focused Entity Map route capabilities.
    pub const fn new(entity_maps: Arc<EntityMapService>, entity_query: Arc<EntityQueryService>) -> EntityMapRouteServices {
        EntityMapRouteServices { entity_maps, entity_query }
    }
}

/// Existing-tenant extraction specialized to Entity Map capabilities.
pub(crate) type EntityMapTenantState = ExistingTenantState<Arc<EntityMapRouteServices>>;

/// Resolves only Entity Map capabilities for one tenant scope.
pub trait EntityMapStateResolver: Send + Sync {
    /// Resolve the tenant according to the supplied creation policy.
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<EntityMapRouteServices>, TenancyError>>;
}

/// Axum state for the complete Entity Map route fragment.
#[derive(Clone)]
pub(crate) struct EntityMapRouteState {
    resolver: Arc<dyn EntityMapStateResolver>,
}

impl EntityMapRouteState {
    pub(crate) const fn new(resolver: Arc<dyn EntityMapStateResolver>) -> EntityMapRouteState {
        EntityMapRouteState { resolver }
    }
}

impl TenantStateResolver<Arc<EntityMapRouteServices>> for EntityMapRouteState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<Arc<EntityMapRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
