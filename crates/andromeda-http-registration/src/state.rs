//! Focused tenant-state resolution for registration routes.

use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    tenant_state::{ExistingTenantState, ProvisionableTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_registration::RegistrationService;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::lease::RuntimeLease, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;

/// The complete registration capability set exposed by one tenant.
pub struct RegistrationRouteServices {
    /// Context-source registration lifecycle policy.
    pub registrations: Arc<RegistrationService>,
    /// The tenant's host of the contexts its responses name when no single
    /// URL names them (Annex C.8).
    pub context_hosting: TenantContextHosting,
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
}

impl RegistrationRouteServices {
    /// Construct the focused capability set.
    #[must_use]
    pub const fn new(registrations: Arc<RegistrationService>, context_hosting: TenantContextHosting, runtime_lease: RuntimeLease) -> RegistrationRouteServices {
        RegistrationRouteServices {
            registrations,
            context_hosting,
            runtime_lease,
        }
    }
}

/// Existing-tenant extraction specialized to registration capabilities.
pub(crate) type RegistrationTenantState = ExistingTenantState<Arc<RegistrationRouteServices>>;
/// Provisionable-tenant extraction specialized to registration capabilities.
pub(crate) type RegistrationTenantStateOrCreate = ProvisionableTenantState<Arc<RegistrationRouteServices>>;

/// Resolves only registration capabilities for one tenant scope.
pub trait RegistrationStateResolver: Send + Sync {
    /// Resolve or create the tenant according to the supplied policy.
    fn resolve<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
    ) -> BoxFuture<'a, Result<Arc<RegistrationRouteServices>, TenancyError>>;
}

/// Axum state for the complete registration route fragment.
#[derive(Clone)]
pub(crate) struct RegistrationRouteState {
    /// Resolves or creates the request's tenant with its registration capabilities.
    resolver: Arc<dyn RegistrationStateResolver>,
}

impl RegistrationRouteState {
    /// Route state resolving tenants through `resolver`.
    pub(crate) const fn new(resolver: Arc<dyn RegistrationStateResolver>) -> RegistrationRouteState {
        RegistrationRouteState { resolver }
    }
}

impl TenantStateResolver<Arc<RegistrationRouteServices>> for RegistrationRouteState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<Arc<RegistrationRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| ProblemResponse::classified(classify_tenancy_error(&error), instance.clone()))
        })
    }
}
