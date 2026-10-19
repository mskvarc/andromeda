//! The tenant registry this crate asks for and does not provide.

use crate::{
    database_name::TenantDatabaseName,
    error::TenancyError,
    provisional::{Establishment, Retirement},
    registered_tenant::RegisteredTenant,
    tenant_page_start::TenantPageStart,
    tenant_record::TenantRecord,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;
use std::time::Duration;

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

    /// Every named tenant currently in the registry, provisional ones included,
    /// by name.
    ///
    /// The source of truth for orphan eviction: a cached runtime whose tenant is
    /// no longer listed has been removed underneath the broker and is holding
    /// connections open for nothing.
    #[must_use = "tenant registry reads must be awaited"]
    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError>;

    /// A named tenant's registry row, if it is registered.
    ///
    /// Carries where the tenant's data lives and whether it exists yet, and for
    /// an established tenant its administrative status, so a cold admission
    /// learns in one read whether the tenant may be served at all: a suspended
    /// tenant is refused rather than provisioned or woken.
    #[must_use = "tenant lookups must be awaited"]
    async fn registered_tenant(&self, name: &TenantName) -> Result<Option<RegisteredTenant>, TenancyError>;

    /// A page of established named-tenant registry rows in name order, with
    /// the collection total, positioned by `start` and capped at `limit`. The
    /// page is returned ascending by name whichever direction it was read in.
    /// Provisional tenants are neither listed nor counted.
    #[must_use = "tenant listings must be awaited"]
    async fn list_tenant_records(&self, start: &TenantPageStart, limit: u32) -> Result<(Vec<TenantRecord>, i64), TenancyError>;

    /// Register the tenant as provisional and prepare its database, or, for a
    /// tenant already registered, make sure its database is prepared.
    ///
    /// The registry row is committed before the database is created, so an
    /// interrupted provisioning leaves a provisional row the broker can settle
    /// and never a database the registry does not name. Provisioning a
    /// provisional tenant again restarts its time to live.
    ///
    /// Must be idempotent and safe against a concurrent caller: two requests
    /// naming the same unknown tenant will both arrive here.
    #[must_use = "tenant provisioning must be awaited"]
    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError>;

    /// Settle a provisional tenant: establish it when its database holds
    /// information.
    ///
    /// Answers [`Establishment::Established`] for a tenant already established
    /// and [`Establishment::Empty`] for a name the registry does not hold.
    /// Serialised against provisioning and retirement of the same tenant.
    #[must_use = "tenant establishment must be awaited"]
    async fn establish(&self, name: &TenantName) -> Result<Establishment, TenancyError>;

    /// Every provisional tenant, by name.
    #[must_use = "tenant registry reads must be awaited"]
    async fn provisional_tenants(&self) -> Result<Vec<TenantName>, TenancyError>;

    /// Retire a provisional tenant that has not been provisioned for `idle_for`
    /// and whose database holds no information: drop its database, then remove
    /// its row.
    ///
    /// The drop is refused while any session is connected to the database, so
    /// a tenant some broker still holds a runtime for is kept. A tenant whose
    /// database holds information is established instead.
    #[must_use = "tenant retirement must be awaited"]
    async fn retire_provisional(&self, name: &TenantName, idle_for: Duration) -> Result<Retirement, TenancyError>;
}
