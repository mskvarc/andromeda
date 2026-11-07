//! Focused tenant-state resolution for Entity Map routes.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    tenant_state::{ExistingTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_temporal::service::TemporalService;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::lease::RuntimeLease, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;

/// The complete Entity Map capability set exposed by one tenant.
pub struct EntityMapRouteServices {
    /// Entity Map lifecycle policy.
    pub entity_maps: Arc<EntityMapService>,
    /// Entity query policy used to materialize map membership.
    pub entity_query: Arc<EntityQueryService>,
    /// Temporal query policy used to materialize temporal map membership
    /// (clause 5.14.5).
    pub temporal: Arc<TemporalService>,
    /// Forwarding to the context sources that contribute their own maps
    /// (clause 5.14.4.4).
    pub forwarding: Arc<FederationService>,
    /// Tenant's live branch; Entity Map management does not accept Snapshot scoping.
    pub live_target: ContextTarget,
    /// The tenant's host of the contexts its responses name when no single
    /// URL names them (Annex C.8).
    pub context_hosting: TenantContextHosting,
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
}

impl EntityMapRouteServices {
    /// Construct focused Entity Map route capabilities.
    #[must_use]
    pub const fn new(
        entity_maps: Arc<EntityMapService>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<FederationService>,
        live_target: ContextTarget,
        context_hosting: TenantContextHosting,
        runtime_lease: RuntimeLease,
    ) -> EntityMapRouteServices {
        EntityMapRouteServices {
            entity_maps,
            entity_query,
            temporal,
            forwarding,
            live_target,
            context_hosting,
            runtime_lease,
        }
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
    /// Resolves the request's tenant to its Entity Map capabilities.
    resolver: Arc<dyn EntityMapStateResolver>,
}

impl EntityMapRouteState {
    /// Route state resolving tenants through `resolver`.
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
