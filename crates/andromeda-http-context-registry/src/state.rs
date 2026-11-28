//! Focused tenant-state resolution for Context Registry routes.

use crate::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::tenant_state::ExistingTenantState;
use andromeda_tenancy::runtime::lease::RuntimeLease;
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
