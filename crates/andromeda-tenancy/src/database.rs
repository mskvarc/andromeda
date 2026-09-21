//! The tenant registry this crate asks for and does not provide.

use crate::{database_name::TenantDatabaseName, error::TenancyError, tenant_keyset::TenantKeyset, tenant_record::TenantRecord};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;

/// Where each tenant's data lives, how a new one is brought into existence, and
/// what the registry knows about each.
///
/// Declared here rather than beside an implementation because the crate that
/// calls a port owns it: whether a tenant is a PostgreSQL database, a schema
/// or a keyspace is the store's decision, and the eviction policy in
/// [`crate::runtime::TenantRuntimeManager`] must not depend on which was made.
#[async_trait]
pub trait TenantDatabase: Send + Sync + 'static {
    /// The database the unnamed default tenant's data lives in.
    ///
    /// Infallible because a broker that cannot answer this has nothing to
    /// serve and never finished starting.
    fn default_database(&self) -> TenantDatabaseName;

    /// Every named tenant currently in the registry, by name.
    ///
    /// The source of truth for orphan eviction: a cached runtime whose tenant is
    /// no longer listed has been removed underneath the broker and is holding
    /// connections open for nothing.
    #[must_use = "tenant registry reads must be awaited"]
    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError>;

    /// A named tenant's registry row, if it is registered.
    ///
    /// Carries both where the tenant's data lives and its administrative status,
    /// so a cold admission learns in one read whether the tenant may be served
    /// at all — a suspended tenant is refused rather than provisioned or woken.
    #[must_use = "tenant lookups must be awaited"]
    async fn tenant_record(&self, name: &TenantName) -> Result<Option<TenantRecord>, TenancyError>;

    /// A page of named-tenant registry rows in name order, with the collection
    /// total, positioned by `keyset` and capped at `limit`.
    #[must_use = "tenant listings must be awaited"]
    async fn list_tenant_records(&self, keyset: &TenantKeyset, limit: i64) -> Result<(Vec<TenantRecord>, i64), TenancyError>;

    /// Register the tenant and prepare its database.
    ///
    /// Must be idempotent and safe against a concurrent caller: two requests
    /// naming the same unknown tenant will both arrive here.
    #[must_use = "tenant provisioning must be awaited"]
    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError>;
}
