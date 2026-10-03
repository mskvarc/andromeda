//! The tenant-management view the broker exposes over HTTP.

use crate::{error::TenancyError, status::TenantStatus, tenant_listing::TenantListing, tenant_page_start::TenantPageStart, tenant_view::TenantView};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;

/// The broker's answer to "what tenants are there, and control one of them".
///
/// Implemented by the runtime cache, because computing a tenant's
/// [`TenantState`](crate::tenant_state::TenantState) needs both the persisted
/// registry row and the cache's own residency, and suspending a tenant means
/// flipping that row and releasing the runtime in one coordinated step. The HTTP
/// tenancy routes hold this and stay thin.
#[async_trait]
pub trait TenantAdministration: Send + Sync + 'static {
    /// One tenant's view, or `None` when no such tenant is registered.
    #[must_use = "tenant reads must be awaited"]
    async fn tenant(&self, name: &TenantName) -> Result<Option<TenantView>, TenancyError>;

    /// The page of the tenant collection `start` and `limit` select, ordered by
    /// name.
    #[must_use = "tenant listings must be awaited"]
    async fn list_tenants(&self, start: &TenantPageStart, limit: u32) -> Result<TenantListing, TenancyError>;

    /// Apply an administrative state to a tenant and return its resulting view,
    /// or `None` when no such tenant is registered.
    ///
    /// Suspending releases the tenant's runtime at once; resuming leaves the next
    /// access to cold-load it.
    #[must_use = "tenant status writes must be awaited"]
    async fn set_admin_status(&self, name: &TenantName, status: TenantStatus) -> Result<Option<TenantView>, TenancyError>;
}
