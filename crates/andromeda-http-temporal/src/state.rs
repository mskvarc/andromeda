//! Focused tenant-state resolution for temporal route handlers.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    context_target::{ContextTargetProvider, ExistingTargetedTenantState, ProvisionableTargetedTenantState, SnapshotTargetResolver},
    tenant_state::TenantStateResolver,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_temporal::service::TemporalService;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::lease::RuntimeLease, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// The complete temporal-only capability set exposed to this binding.
#[derive(TypedBuilder)]
pub struct TemporalRouteServices {
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
    /// Temporal lifecycle and history policy.
    pub temporal: Arc<TemporalService>,
    /// Distributed temporal forwarding policy.
    pub forwarding: Arc<FederationService>,
    /// Entity-map policy used for temporal query scoping and result-map creation.
    pub entity_maps: Arc<EntityMapService>,
    /// Tenant's ordinary live context branch.
    pub live_target: ContextTarget,
    /// Snapshot URI to durable branch resolution.
    pub snapshot_targets: Arc<dyn SnapshotTargetResolver>,
}

/// Existing-tenant extractor specialized to focused temporal capabilities.
pub(crate) type TemporalTenantState = ExistingTargetedTenantState<Arc<TemporalRouteServices>>;

/// Provisionable-tenant extractor specialized to focused temporal capabilities.
pub(crate) type TemporalTenantStateOrCreate = ProvisionableTargetedTenantState<Arc<TemporalRouteServices>>;

impl ContextTargetProvider for TemporalRouteServices {
    fn live_target(&self) -> &ContextTarget {
        &self.live_target
    }

    fn snapshot_target_resolver(&self) -> &dyn SnapshotTargetResolver {
        self.snapshot_targets.as_ref()
    }
}

/// Resolves only the temporal capabilities for one tenant scope.
pub trait TemporalStateResolver: Send + Sync {
    /// Resolve or create the tenant according to the supplied policy.
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<TemporalRouteServices>, TenancyError>>;
}

/// Axum state held by the state-complete temporal fragment.
#[derive(Clone)]
pub(crate) struct TemporalRouteState {
    /// Focused composition-supplied resolver.
    resolver: Arc<dyn TemporalStateResolver>,
}

impl TemporalRouteState {
    /// Construct route state around one focused resolver.
    pub(crate) const fn new(resolver: Arc<dyn TemporalStateResolver>) -> TemporalRouteState {
        TemporalRouteState { resolver }
    }
}

impl TenantStateResolver<Arc<TemporalRouteServices>> for TemporalRouteState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<Arc<TemporalRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
