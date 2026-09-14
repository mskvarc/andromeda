//! The tunables that decide how many tenant runtimes stay open, and for how
//! long.

use crate::db_name_prefix::DbNamePrefix;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid tenancy provisioning or runtime-cache configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TenancyConfigError {
    /// Per-tenant pool size was configured as zero.
    #[error("tenancy.tenant_max_connections must be at least 1")]
    ZeroTenantMaxConnections,
    /// Runtime idle retention was configured as zero.
    #[error("tenancy.runtime_idle_ttl_secs must be at least 1")]
    ZeroRuntimeIdleTtl,
    /// Runtime cache capacity was configured as zero.
    #[error("tenancy.runtime_max_cached must be at least 1")]
    ZeroRuntimeMaxCached,
    /// Runtime janitor cadence was configured as zero.
    #[error("tenancy.janitor_interval_secs must be at least 1")]
    ZeroJanitorInterval,
}

/// Multi-tenancy configuration.
///
/// Two of these settings are read by this crate and two by whichever store
/// provisions a tenant: `tenant_db_prefix` and `tenant_max_connections` mean
/// nothing to a runtime cache. They live together because they are one
/// operator-facing section, not because one type uses them all.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct TenancyConfig {
    /// Whether named tenants are served at all.
    pub enabled: bool,

    /// Prefix for the per-tenant database names the store provisions.
    #[toml_comment(inline)]
    pub tenant_db_prefix: DbNamePrefix,

    /// Maximum number of connections in per-tenant database pools.
    ///
    /// Each tenant gets its own connection pool capped at this value,
    /// preventing a single tenant from exhausting system-wide connection
    /// limits.
    pub tenant_max_connections: u32,

    /// How long an unused tenant runtime is held open before eviction.
    pub runtime_idle_ttl_secs: u64,

    /// How many tenant runtimes may be held open at once.
    pub runtime_max_cached: usize,

    /// How often the janitor sweeps for idle and orphaned runtimes.
    pub janitor_interval_secs: u64,
}

impl Default for TenancyConfig {
    fn default() -> TenancyConfig {
        TenancyConfig {
            enabled: true,
            tenant_db_prefix: DbNamePrefix::default(),
            tenant_max_connections: 16,
            runtime_idle_ttl_secs: 900,
            runtime_max_cached: 64,
            janitor_interval_secs: 30,
        }
    }
}

impl TenancyConfig {
    /// Validate tenant pool and runtime-cache limits.
    ///
    /// # Errors
    ///
    /// Returns the [`TenancyConfigError`] naming the first setting configured
    /// as zero: the per-tenant pool size, the runtime idle retention, the
    /// runtime cache capacity or the janitor cadence.
    pub const fn validate(&self) -> Result<(), TenancyConfigError> {
        if self.tenant_max_connections == 0 {
            return Err(TenancyConfigError::ZeroTenantMaxConnections);
        }
        if self.runtime_idle_ttl_secs == 0 {
            return Err(TenancyConfigError::ZeroRuntimeIdleTtl);
        }
        if self.runtime_max_cached == 0 {
            return Err(TenancyConfigError::ZeroRuntimeMaxCached);
        }
        if self.janitor_interval_secs == 0 {
            return Err(TenancyConfigError::ZeroJanitorInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod config_tests {
    use super::{TenancyConfig, TenancyConfigError};
    use crate::db_name_prefix::DbNamePrefix;
    use pretty_assertions::assert_eq;

    #[test]
    fn zero_runtime_cache_capacity_is_rejected() {
        let config = TenancyConfig {
            enabled: true,
            tenant_db_prefix: DbNamePrefix::try_from("andromeda_tenant").expect("database prefix"),
            tenant_max_connections: 16,
            runtime_idle_ttl_secs: 900,
            runtime_max_cached: 0,
            janitor_interval_secs: 30,
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroRuntimeMaxCached));
    }
}
