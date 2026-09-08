//! Focused tenant-state resolution for snapshot routes.

use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    tenant_state::{ExistingTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_snapshot::service::SnapshotService;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;

/// The complete snapshot capability set exposed by one tenant.
pub struct SnapshotRouteServices {
    /// Snapshot lifecycle policy.
    pub snapshots: Arc<SnapshotService>,
}

impl SnapshotRouteServices {
    /// Construct focused snapshot capabilities.
    #[must_use]
    pub const fn new(snapshots: Arc<SnapshotService>) -> SnapshotRouteServices {
        SnapshotRouteServices { snapshots }
    }
}

/// Existing-tenant extraction specialized to snapshot capabilities.
pub(crate) type SnapshotTenantState = ExistingTenantState<Arc<SnapshotRouteServices>>;

/// Resolves only snapshot capabilities for one tenant scope.
pub trait SnapshotStateResolver: Send + Sync {
    /// Resolve the tenant according to the supplied policy.
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<SnapshotRouteServices>, TenancyError>>;
}

#[derive(Clone)]
pub(crate) struct SnapshotRouteState {
    resolver: Arc<dyn SnapshotStateResolver>,
}

impl SnapshotRouteState {
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
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
