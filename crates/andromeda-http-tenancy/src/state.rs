//! Focused tenant-management route state.

use andromeda_tenancy::{administration::TenantAdministration, usage_probe::TenantUsageProbe};
use std::sync::Arc;

/// State exposing the tenant-administration view and the opt-in usage probe.
///
/// Two capabilities from two owners: the administration port (implemented by the
/// runtime cache) answers the cheap-by-default reads and the suspend/resume
/// write, while the usage probe (implemented by the store) is consulted only when
/// a request asks for `?options=usage`.
#[derive(Clone)]
pub struct TenancyRouteState {
    /// The tenant listing, introspection and lifecycle capability.
    administration: Arc<dyn TenantAdministration>,
    /// The opt-in operational-usage capability.
    usage: Arc<dyn TenantUsageProbe>,
}

impl TenancyRouteState {
    /// Construct focused tenant-management state.
    pub const fn new(administration: Arc<dyn TenantAdministration>, usage: Arc<dyn TenantUsageProbe>) -> TenancyRouteState {
        TenancyRouteState { administration, usage }
    }

    /// Borrow the tenant-administration capability.
    #[must_use]
    pub fn administration(&self) -> &dyn TenantAdministration {
        self.administration.as_ref()
    }

    /// Borrow the operational-usage capability.
    #[must_use]
    pub fn usage(&self) -> &dyn TenantUsageProbe {
        self.usage.as_ref()
    }
}
