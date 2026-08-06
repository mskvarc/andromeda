//! The tunables that decide how many tenant runtimes stay open, and for how
//! long.

use confique::Config;
use serde::Serialize;

/// Multi-tenancy configuration.
///
/// Two of these settings are read by this crate and two by whichever store
/// provisions a tenant: `tenant_db_prefix` and `tenant_max_connections` mean
/// nothing to a runtime cache. They live together because they are one
/// operator-facing section, not because one type uses them all.
#[derive(Config, Debug, Clone, Serialize)]
pub struct TenancyConfig {
    /// Whether named tenants are served at all.
    #[config(env = "ANDROMEDA_TENANCY_ENABLED", default = true)]
    pub enabled: bool,

    /// Prefix for the per-tenant database names the store provisions.
    #[config(env = "ANDROMEDA_TENANCY_TENANT_DB_PREFIX", default = "andromeda_tenant")]
    pub tenant_db_prefix: String,

    /// Maximum number of connections in per-tenant database pools.
    ///
    /// Each tenant gets its own connection pool capped at this value,
    /// preventing a single tenant from exhausting system-wide connection
    /// limits.
    #[config(env = "ANDROMEDA_TENANCY_TENANT_MAX_CONNECTIONS", default = 16)]
    pub tenant_max_connections: u32,

    /// How long an unused tenant runtime is held open before eviction.
    #[config(env = "ANDROMEDA_TENANCY_RUNTIME_IDLE_TTL_SECONDS", default = 900)]
    pub runtime_idle_ttl_seconds: u64,

    /// How many tenant runtimes may be held open at once.
    #[config(env = "ANDROMEDA_TENANCY_RUNTIME_MAX_CACHED", default = 64)]
    pub runtime_max_cached: usize,

    /// How often the janitor sweeps for idle and orphaned runtimes.
    #[config(env = "ANDROMEDA_TENANCY_JANITOR_INTERVAL_SECONDS", default = 30)]
    pub janitor_interval_seconds: u64,
}
