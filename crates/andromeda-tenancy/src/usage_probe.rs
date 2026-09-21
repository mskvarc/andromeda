//! Reading a tenant's database for its operational usage.

use crate::{database_name::TenantDatabaseName, error::TenancyError, tenant_usage::TenantUsage};
use async_trait::async_trait;

/// Gathers a tenant's operational usage from its own database.
///
/// A port this crate asks for and does not provide: the runtime cache knows
/// nothing of how usage is measured, only that measuring it costs a tenant-DB
/// round trip, which is why an introspection read gathers it only on request.
/// Implemented over a transient connection to the tenant's database, so it adds
/// no permanent backend and never makes the tenant resident.
#[async_trait]
pub trait TenantUsageProbe: Send + Sync + 'static {
    /// Read the tenant database's current operational usage.
    #[must_use = "tenant usage reads must be awaited"]
    async fn usage(&self, database: &TenantDatabaseName) -> Result<TenantUsage, TenancyError>;
}
