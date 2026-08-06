//! The tenant registry this crate asks for and does not provide.

use crate::{database_name::TenantDatabaseName, error::TenancyError};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;

/// Where each tenant's data lives, and how a new one is brought into
/// existence.
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

    /// Every named tenant currently in the registry.
    ///
    /// Also the source of truth for eviction: a cached runtime whose tenant is
    /// no longer listed has been removed underneath the broker and is holding
    /// connections open for nothing.
    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError>;

    /// The database a named tenant's data lives in, if it is registered.
    async fn lookup(&self, name: &TenantName) -> Result<Option<TenantDatabaseName>, TenancyError>;

    /// Register the tenant and prepare its database.
    ///
    /// Must be idempotent and safe against a concurrent caller: two requests
    /// naming the same unknown tenant will both arrive here.
    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError>;
}
