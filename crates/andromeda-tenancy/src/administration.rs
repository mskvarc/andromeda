//! Administrative capabilities over the tenant registry and runtime cache.

use crate::error::TenancyError;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;

/// Lists the named tenants visible through the registry.
#[async_trait]
pub trait TenantDirectory: Send + Sync + 'static {
    /// Return every registered named tenant.
    #[must_use = "tenant directory reads must be awaited"]
    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError>;
}

/// Evicts cached runtimes whose tenant is no longer registered.
#[async_trait]
pub trait OrphanTenantEvictor: Send + Sync + 'static {
    /// Perform one orphan sweep and return the number of released runtimes.
    #[must_use = "orphan eviction must be awaited"]
    async fn evict_orphan_runtimes(&self) -> usize;
}
