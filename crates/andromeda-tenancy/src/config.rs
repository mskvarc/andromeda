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
    /// The concurrent cold-build admission bound was configured as zero.
    #[error("tenancy.max_concurrent_builds must be at least 1")]
    ZeroMaxConcurrentBuilds,
    /// The concurrent-eviction bound was configured as zero.
    #[error("tenancy.eviction_concurrency must be at least 1")]
    ZeroEvictionConcurrency,
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
    /// Each tenant gets its own connection pool capped at this value. Combined
    /// with `runtime_max_cached`, which caps how many per-tenant runtimes stay
    /// open at once, this bounds the live connection count at
    /// `runtime_max_cached * tenant_max_connections` (plus the platform pools),
    /// so tenant churn cannot exhaust the server's connection limit.
    pub tenant_max_connections: u32,

    /// How long an unused tenant runtime is held open before eviction.
    pub runtime_idle_ttl_secs: u64,

    /// How many tenant runtimes may be held open at once.
    ///
    /// The cap is enforced both on the janitor tick and synchronously when a
    /// cold build inserts a runtime, so `runtime_max_cached *
    /// tenant_max_connections` bounds the live per-tenant pool count between
    /// sweeps rather than letting it climb until the next tick.
    ///
    /// The bound is honoured for runtimes nobody is using. A runtime a request
    /// currently holds is leased and never evicted, so while more than
    /// `runtime_max_cached` distinct tenants have requests in flight at once the
    /// cache overshoots this cap transiently; the overshoot is bounded by the
    /// number of concurrent requests and is trimmed back by the next sweep or
    /// insert as those requests complete and drop their leases.
    pub runtime_max_cached: usize,

    /// How often the janitor sweeps for idle and orphaned runtimes.
    pub janitor_interval_secs: u64,

    /// How many tenant runtimes may be cold-built at once.
    ///
    /// A cold build opens a connection pool and, on a schema that is behind or
    /// absent, runs the migrator over a direct connection. Without a bound a
    /// burst of first requests for distinct tenants would open that many pools
    /// and migrator connections at once and exhaust the server. Requests past
    /// the bound wait for a build permit rather than being refused; the wait is
    /// reported as `andromeda_tenant_build_admission_wait_seconds`.
    pub max_concurrent_builds: usize,

    /// How many tenant runtimes may be torn down at once.
    ///
    /// Eviction closes a runtime's connection pool and stops its workers, which
    /// is I/O the request that triggered a cache trim must not serialise behind.
    /// Teardowns run concurrently up to this bound, off the request path, so a
    /// cold insert never waits on another tenant's shutdown.
    pub eviction_concurrency: usize,
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
            max_concurrent_builds: 8,
            eviction_concurrency: 8,
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
    /// runtime cache capacity, the janitor cadence, the concurrent cold-build
    /// bound or the concurrent-eviction bound.
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
        if self.max_concurrent_builds == 0 {
            return Err(TenancyConfigError::ZeroMaxConcurrentBuilds);
        }
        if self.eviction_concurrency == 0 {
            return Err(TenancyConfigError::ZeroEvictionConcurrency);
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
            max_concurrent_builds: 8,
            eviction_concurrency: 8,
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroRuntimeMaxCached));
    }

    #[test]
    fn a_zero_concurrent_build_bound_is_rejected() {
        let config = TenancyConfig {
            max_concurrent_builds: 0,
            ..TenancyConfig::default()
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroMaxConcurrentBuilds));
    }

    #[test]
    fn a_zero_eviction_concurrency_is_rejected() {
        let config = TenancyConfig {
            eviction_concurrency: 0,
            ..TenancyConfig::default()
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroEvictionConcurrency));
    }
}
