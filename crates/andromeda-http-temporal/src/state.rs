//! Focused tenant-state resolution for temporal route handlers.

use crate::{error::classify_tenancy_error, snapshot_scope::TemporalSnapshotScope};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_extraction::tenant_state::{ExistingTenantState, ProvisionableTenantState, TenantStateResolver};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_temporal::service::TemporalService;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
use futures::future::BoxFuture;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// The complete temporal-only capability set exposed to this binding.
#[derive(TypedBuilder)]
pub struct TemporalRouteServices {
    /// Temporal lifecycle and history policy.
    pub temporal: Arc<TemporalService>,
    /// Distributed temporal forwarding policy.
    pub forwarding: Arc<FederationService>,
    /// Entity-map policy used for temporal query scoping and result-map creation.
    pub entity_maps: Arc<EntityMapService>,
    /// Frozen temporal reads used by snapshot-scoped requests.
    pub snapshots: Arc<dyn TemporalSnapshotScope>,
}

/// Existing-tenant extractor specialized to focused temporal capabilities.
pub(crate) type TemporalTenantState = ExistingTenantState<Arc<TemporalRouteServices>>;

/// Provisionable-tenant extractor specialized to focused temporal capabilities.
pub(crate) type TemporalTenantStateOrCreate = ProvisionableTenantState<Arc<TemporalRouteServices>>;

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
        instance: &'a axum::http::Uri,
    ) -> BoxFuture<'a, Result<Arc<TemporalRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
