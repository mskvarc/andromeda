//! Focused tenant-administration route state.

use andromeda_tenancy::administration::{OrphanTenantEvictor, TenantDirectory};
use std::sync::Arc;

/// State exposing only tenant listing and orphan-runtime eviction.
#[derive(Clone)]
pub struct TenancyRouteState {
    /// Registered-tenant query capability.
    directory: Arc<dyn TenantDirectory>,
    /// Orphan-runtime eviction capability.
    evictor: Arc<dyn OrphanTenantEvictor>,
}

impl TenancyRouteState {
    /// Construct focused tenant-administration state.
    pub const fn new(directory: Arc<dyn TenantDirectory>, evictor: Arc<dyn OrphanTenantEvictor>) -> TenancyRouteState {
        TenancyRouteState { directory, evictor }
    }

    /// Borrow the registered-tenant query capability.
    pub fn directory(&self) -> &dyn TenantDirectory {
        self.directory.as_ref()
    }

    /// Borrow the orphan-runtime eviction capability.
    pub fn evictor(&self) -> &dyn OrphanTenantEvictor {
        self.evictor.as_ref()
    }
}
