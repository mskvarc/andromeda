//! Focused tenant-state resolution for snapshot routes.

use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    tenant_state::{ExistingTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_snapshot::service::SnapshotService;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::lease::RuntimeLease, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;

/// The complete snapshot capability set exposed by one tenant.
pub struct SnapshotRouteServices {
    /// Snapshot lifecycle policy.
    pub snapshots: Arc<SnapshotService>,
    /// The tenant's JSON-LD engine, which admits the user `@context` a
    /// Snapshot is created under (clause 5.5.7).
    pub engine: JsonLdEngine,
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
}

impl SnapshotRouteServices {
    /// Construct focused snapshot capabilities.
    #[must_use]
    pub const fn new(snapshots: Arc<SnapshotService>, engine: JsonLdEngine, runtime_lease: RuntimeLease) -> SnapshotRouteServices {
        SnapshotRouteServices {
            snapshots,
            engine,
            runtime_lease,
        }
    }
}

/// Existing-tenant extraction specialized to snapshot capabilities.
pub(crate) type SnapshotTenantState = ExistingTenantState<Arc<SnapshotRouteServices>>;

/// Resolves only snapshot capabilities for one tenant scope.
pub trait SnapshotStateResolver: Send + Sync {
    /// Resolve the tenant according to the supplied policy.
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<SnapshotRouteServices>, TenancyError>>;
}

/// Axum state for the complete snapshot route fragment.
#[derive(Clone)]
pub(crate) struct SnapshotRouteState {
    /// Resolves the request's tenant to its snapshot capabilities.
    resolver: Arc<dyn SnapshotStateResolver>,
}

impl SnapshotRouteState {
    /// Route state resolving tenants through `resolver`.
    pub(crate) const fn new(resolver: Arc<dyn SnapshotStateResolver>) -> SnapshotRouteState {
        SnapshotRouteState { resolver }
    }
}

impl TenantStateResolver<Arc<SnapshotRouteServices>> for SnapshotRouteState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<Arc<SnapshotRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| ProblemResponse::classified(classify_tenancy_error(&error), instance.clone()))
        })
    }
}
