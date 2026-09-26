//! The tunables that decide how many tenant runtimes stay open, and for how
//! long.

use crate::db_name_prefix::DbNamePrefix;
use andromeda_configuration_scalars::human_duration::HumanDuration;
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
    #[error("tenancy.runtime_idle_ttl must be greater than zero")]
    ZeroRuntimeIdleTtl,
    /// Runtime cache capacity was configured as zero.
    #[error("tenancy.runtime_max_cached must be at least 1")]
    ZeroRuntimeMaxCached,
    /// Runtime janitor cadence was configured as zero.
    #[error("tenancy.janitor_interval must be greater than zero")]
    ZeroJanitorInterval,
    /// The concurrent cold-build admission bound was configured as zero.
    #[error("tenancy.max_concurrent_builds must be at least 1")]
    ZeroMaxConcurrentBuilds,
    /// The concurrent-eviction bound was configured as zero.
    #[error("tenancy.eviction_concurrency must be at least 1")]
    ZeroEvictionConcurrency,
    /// The dormant-tenant re-wake floor was configured as zero.
    #[error("tenancy.min_dormancy must be greater than zero")]
    ZeroMinDormancy,
    /// The woken-runtime residency was configured as zero.
    #[error("tenancy.wake_residency must be greater than zero")]
    ZeroWakeResidency,
    /// The per-pass wake bound was configured as zero.
    #[error("tenancy.waker_max_wakes_per_pass must be at least 1")]
    ZeroWakerMaxWakesPerPass,
    /// The cold-build admission deadline was configured as zero.
    #[error("tenancy.build_admission_timeout must be greater than zero")]
    ZeroBuildAdmissionTimeout,
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

    /// How long an unused tenant runtime is held open before eviction, in
    /// human-readable form (`"15m"`).
    #[toml_comment(inline)]
    pub runtime_idle_ttl: HumanDuration,

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

    /// How often the janitor sweeps for idle and orphaned runtimes, in
    /// human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub janitor_interval: HumanDuration,

    /// How many tenant runtimes may be cold-built at once.
    ///
    /// A cold build opens a connection pool and, on a schema that is behind or
    /// absent, runs the migrator over a direct connection. Without a bound a
    /// burst of first requests for distinct tenants would open that many pools
    /// and migrator connections at once and exhaust the server. Requests past
    /// the bound wait for a build permit for at most `build_admission_timeout`;
    /// the wait is reported as `andromeda_tenant_build_admission_wait_seconds`.
    pub max_concurrent_builds: usize,

    /// How long a request may wait to be admitted to a cold build before it is
    /// refused as saturated, in human-readable form (`"20s"`).
    ///
    /// Covers every wait ahead of the build itself: another request's in-flight
    /// build of the same tenant, a build permit under `max_concurrent_builds`,
    /// and a teardown slot under `eviction_concurrency` when the cache is full
    /// and a runtime must be released first. A request that cannot be admitted
    /// within it is answered `503` with `Retry-After` rather than stalling until
    /// the HTTP request timeout, which it must therefore stay below.
    #[toml_comment(inline)]
    pub build_admission_timeout: HumanDuration,

    /// How many tenant runtimes may be torn down at once.
    ///
    /// Eviction closes a runtime's connection pool and stops its workers. Every
    /// eviction path takes a teardown slot before it removes a runtime from the
    /// cache, so a runtime is either cached or in one of these slots and the
    /// runtimes alive at once stay within `runtime_max_cached` plus this bound,
    /// beyond the transient overshoot of runtimes leased by in-flight requests
    /// (see `runtime_max_cached`) or still being built. A cold build
    /// that must make room while every slot is busy waits for one within
    /// `build_admission_timeout`.
    pub eviction_concurrency: usize,

    /// The shortest delay before a just-evicted dormant tenant is woken again.
    ///
    /// When the janitor evicts a tenant it probes for the earliest instant the
    /// tenant has work due and records a wake for it. A due instant sooner than
    /// `now + min_dormancy` is clamped up to it, so evicting a tenant that
    /// is only a little overdue and immediately reloading it cannot thrash. A
    /// failed probe records a retry this far out rather than leaving the tenant
    /// unscheduled. Human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub min_dormancy: HumanDuration,

    /// How long a woken runtime is held resident to do its due work.
    ///
    /// A runtime the waker loads exists only to drain the work its probe found.
    /// Its last use is backdated so it is the first idle-eviction victim yet
    /// keeps this much idle life, so it never displaces a runtime a request
    /// touched. Must be below `runtime_idle_ttl` to have that effect.
    /// Human-readable form (`"1m"`).
    #[toml_comment(inline)]
    pub wake_residency: HumanDuration,

    /// How many dormant tenants a single wake pass loads at most.
    ///
    /// Woken runtimes are cold-built through the same admission bound as request
    /// cold builds; this caps how much of a pass's budget the waker takes so a
    /// wake burst cannot starve request-driven cold loads.
    pub waker_max_wakes_per_pass: usize,
}

impl Default for TenancyConfig {
    fn default() -> TenancyConfig {
        TenancyConfig {
            enabled: true,
            tenant_db_prefix: DbNamePrefix::default(),
            tenant_max_connections: 16,
            runtime_idle_ttl: HumanDuration::from_secs(900),
            runtime_max_cached: 64,
            janitor_interval: HumanDuration::from_secs(30),
            max_concurrent_builds: 8,
            build_admission_timeout: HumanDuration::from_secs(20),
            eviction_concurrency: 8,
            min_dormancy: HumanDuration::from_secs(30),
            wake_residency: HumanDuration::from_secs(60),
            waker_max_wakes_per_pass: 4,
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
    /// bound, the cold-build admission deadline, the concurrent-eviction bound,
    /// the dormant re-wake floor, the woken-runtime residency or the per-pass
    /// wake bound.
    pub const fn validate(&self) -> Result<(), TenancyConfigError> {
        if self.tenant_max_connections == 0 {
            return Err(TenancyConfigError::ZeroTenantMaxConnections);
        }
        if self.runtime_idle_ttl.is_zero() {
            return Err(TenancyConfigError::ZeroRuntimeIdleTtl);
        }
        if self.runtime_max_cached == 0 {
            return Err(TenancyConfigError::ZeroRuntimeMaxCached);
        }
        if self.janitor_interval.is_zero() {
            return Err(TenancyConfigError::ZeroJanitorInterval);
        }
        if self.max_concurrent_builds == 0 {
            return Err(TenancyConfigError::ZeroMaxConcurrentBuilds);
        }
        if self.build_admission_timeout.is_zero() {
            return Err(TenancyConfigError::ZeroBuildAdmissionTimeout);
        }
        if self.eviction_concurrency == 0 {
            return Err(TenancyConfigError::ZeroEvictionConcurrency);
        }
        if self.min_dormancy.is_zero() {
            return Err(TenancyConfigError::ZeroMinDormancy);
        }
        if self.wake_residency.is_zero() {
            return Err(TenancyConfigError::ZeroWakeResidency);
        }
        if self.waker_max_wakes_per_pass == 0 {
            return Err(TenancyConfigError::ZeroWakerMaxWakesPerPass);
        }
        Ok(())
    }
}

#[cfg(test)]
mod config_tests {
    use super::{TenancyConfig, TenancyConfigError};
    use crate::db_name_prefix::DbNamePrefix;
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;

    #[test]
    fn zero_runtime_cache_capacity_is_rejected() {
        let config = TenancyConfig {
            enabled: true,
            tenant_db_prefix: DbNamePrefix::try_from("andromeda_tenant").expect("database prefix"),
            tenant_max_connections: 16,
            runtime_idle_ttl: HumanDuration::from_secs(900),
            runtime_max_cached: 0,
            janitor_interval: HumanDuration::from_secs(30),
            max_concurrent_builds: 8,
            build_admission_timeout: HumanDuration::from_secs(20),
            eviction_concurrency: 8,
            min_dormancy: HumanDuration::from_secs(30),
            wake_residency: HumanDuration::from_secs(60),
            waker_max_wakes_per_pass: 4,
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
    fn a_zero_build_admission_timeout_is_rejected() {
        let config = TenancyConfig {
            build_admission_timeout: HumanDuration::from_secs(0),
            ..TenancyConfig::default()
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroBuildAdmissionTimeout));
    }

    #[test]
    fn a_zero_eviction_concurrency_is_rejected() {
        let config = TenancyConfig {
            eviction_concurrency: 0,
            ..TenancyConfig::default()
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroEvictionConcurrency));
    }

    #[test]
    fn a_zero_min_dormancy_is_rejected() {
        let config = TenancyConfig {
            min_dormancy: HumanDuration::from_secs(0),
            ..TenancyConfig::default()
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroMinDormancy));
    }

    #[test]
    fn a_zero_wake_residency_is_rejected() {
        let config = TenancyConfig {
            wake_residency: HumanDuration::from_secs(0),
            ..TenancyConfig::default()
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroWakeResidency));
    }

    #[test]
    fn a_zero_per_pass_wake_bound_is_rejected() {
        let config = TenancyConfig {
            waker_max_wakes_per_pass: 0,
            ..TenancyConfig::default()
        };
        assert_eq!(config.validate(), Err(TenancyConfigError::ZeroWakerMaxWakesPerPass));
    }
}
