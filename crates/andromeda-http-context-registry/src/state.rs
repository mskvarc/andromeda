//! Focused tenant-state resolution for Context Registry routes.

use crate::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    tenant_state::{ExistingTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::lease::RuntimeLease, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;

/// The complete Context Registry capability set exposed by one tenant.
pub struct ContextRegistryRouteServices {
    /// The tenant's stored `@context`s, whose lifecycle the routes drive and
    /// whose serving URLs they mint.
    pub hosting: TenantContextHosting,
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
}

impl ContextRegistryRouteServices {
    /// Construct the focused capability set.
    #[must_use]
    pub const fn new(hosting: TenantContextHosting, runtime_lease: RuntimeLease) -> ContextRegistryRouteServices {
        ContextRegistryRouteServices { hosting, runtime_lease }
    }
}

/// Existing-tenant extraction specialized to Context Registry capabilities.
pub(crate) type ContextRegistryTenantState = ExistingTenantState<Arc<ContextRegistryRouteServices>>;

/// Resolves only Context Registry capabilities for one tenant scope.
pub trait ContextRegistryStateResolver: Send + Sync {
    /// Resolve the tenant according to the supplied creation policy.
    fn resolve<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
    ) -> BoxFuture<'a, Result<Arc<ContextRegistryRouteServices>, TenancyError>>;
}

/// Axum state for the complete Context Registry route fragment.
#[derive(Clone)]
pub(crate) struct ContextRegistryRouteState {
    /// Resolves the request's tenant to its Context Registry capabilities.
    resolver: Arc<dyn ContextRegistryStateResolver>,
}

impl ContextRegistryRouteState {
    /// Route state resolving tenants through `resolver`.
    pub(crate) const fn new(resolver: Arc<dyn ContextRegistryStateResolver>) -> ContextRegistryRouteState {
        ContextRegistryRouteState { resolver }
    }
}

impl TenantStateResolver<Arc<ContextRegistryRouteServices>> for ContextRegistryRouteState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<Arc<ContextRegistryRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
