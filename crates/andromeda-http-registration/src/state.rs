//! Focused tenant-state resolution for registration routes.

use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::tenant_state::{ExistingTenantState, ProvisionableTenantState};
use andromeda_registration::RegistrationService;
use andromeda_tenancy::runtime::lease::RuntimeLease;
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
