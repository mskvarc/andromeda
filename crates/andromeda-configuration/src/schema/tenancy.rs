//! The `[tenancy]` table: whether named tenants are served, where their data
//! lives, how many stay open, and how one is opened.

use crate::{
    human_duration::HumanDuration,
    schema::{db_name_prefix::DbNamePrefix, feature_mode::FeatureMode},
};
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU32, NonZeroUsize};
use toml_comment::TomlComment;

/// Where each named tenant's data lives.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TenantDatabasesConfig {
    /// The prefix of every per-tenant database name the broker provisions.
    #[toml_comment(inline)]
    pub name_prefix: DbNamePrefix,
    /// The most connections one tenant's pool opens.
    ///
    /// With `tenancy.residency.max_resident`, which caps how many tenants
    /// stay open at once, this bounds the live connection count at
    /// `max_resident * max_connections` (plus the platform pools), so tenant
    /// churn cannot exhaust the server's connection limit.
    #[toml_comment(inline)]
    pub max_connections: NonZeroU32,
}

impl Default for TenantDatabasesConfig {
    fn default() -> TenantDatabasesConfig {
        TenantDatabasesConfig {
            name_prefix: DbNamePrefix::default(),
            max_connections: NonZeroU32::MIN.saturating_add(15),
        }
    }
}

/// How many tenant runtimes stay open, and for how long.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResidencyConfig {
    /// How long an unused tenant runtime stays open before it is evicted, in
    /// human-readable form (`"15m"`).
    #[toml_comment(inline)]
    pub idle_eviction_after: HumanDuration,
    /// The most tenant runtimes open at once.
    ///
    /// Enforced on every sweep and whenever a cold build adds a runtime. A
    /// runtime a request currently holds is never evicted, so while more
    /// tenants than this have requests in flight at once the cache overshoots
    /// it by at most the number of concurrent requests, trimmed back as they
    /// complete.
    #[toml_comment(inline)]
    pub max_resident: NonZeroUsize,
    /// How often idle and orphaned runtimes are swept, in human-readable form
    /// (`"30s"`).
    #[toml_comment(inline)]
    pub sweep_interval: HumanDuration,
    /// The shortest wait before a just-evicted tenant with due work is woken
    /// again, in human-readable form (`"30s"`).
    ///
    /// When a tenant is evicted its earliest due work is probed and a wake
    /// recorded for it; a due instant sooner than this is pushed back to it,
    /// so evicting a tenant that is only a little overdue and reloading it
    /// cannot thrash. A failed probe records a retry this far out.
    #[toml_comment(inline)]
    pub min_wake_interval: HumanDuration,
    /// How long a runtime woken for its due work stays open, in
    /// human-readable form (`"1m"`). It is the first idle-eviction candidate
    /// yet keeps this much idle life, so it never displaces a runtime a
    /// request touched. Below `idle_eviction_after` to have that effect.
    #[toml_comment(inline)]
    pub woken_lifetime: HumanDuration,
    /// The most dormant tenants one wake pass opens. Woken runtimes are
    /// built through the same admission bound as request cold builds, so this
    /// keeps a wake burst from starving request-driven cold loads.
    #[toml_comment(inline)]
    pub max_wakes_per_pass: NonZeroUsize,
    /// The most tenant runtimes torn down at once.
    ///
    /// Eviction closes a runtime's pool and stops its workers; every eviction
    /// takes a teardown slot first, so the runtimes alive at once stay within
    /// `max_resident` plus this bound. A cold build that must make room while
    /// every slot is busy waits for one within `tenancy.build.admission_timeout`.
    #[toml_comment(inline)]
    pub max_concurrent_evictions: NonZeroUsize,
}

impl Default for ResidencyConfig {
    fn default() -> ResidencyConfig {
        ResidencyConfig {
            idle_eviction_after: HumanDuration::from_secs(900),
            max_resident: NonZeroUsize::MIN.saturating_add(63),
            sweep_interval: HumanDuration::from_secs(30),
            min_wake_interval: HumanDuration::from_secs(30),
            woken_lifetime: HumanDuration::from_secs(60),
            max_wakes_per_pass: NonZeroUsize::MIN.saturating_add(3),
            max_concurrent_evictions: NonZeroUsize::MIN.saturating_add(7),
        }
    }
}

/// How a tenant runtime is opened on its first request.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TenantBuildConfig {
    /// The most tenant runtimes built at once.
    ///
    /// A cold build opens a connection pool and, on a schema that is behind or
    /// absent, runs the migrator over a direct connection. Requests past the
    /// bound wait for a build permit for at most `admission_timeout`; the wait
    /// is reported as `andromeda_tenant_build_admission_wait_seconds`.
    #[toml_comment(inline)]
    pub max_concurrent: NonZeroUsize,
    /// How long a request may wait to be admitted to a cold build before it is
    /// answered 503 with `Retry-After`, in human-readable form (`"20s"`).
    ///
    /// Covers every wait ahead of the build itself: another request's build of
    /// the same tenant, a build permit, and a teardown slot when the cache is
    /// full. Below `request_limits.timeout`, which it would otherwise race.
    #[toml_comment(inline)]
    pub admission_timeout: HumanDuration,
}

impl Default for TenantBuildConfig {
    fn default() -> TenantBuildConfig {
        TenantBuildConfig {
            max_concurrent: NonZeroUsize::MIN.saturating_add(7),
            admission_timeout: HumanDuration::from_secs(20),
        }
    }
}

/// Whether named tenants are served, where their data lives, how many stay
/// open, and how one is opened.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct TenancyConfig {
    /// Whether named tenants are served: `enabled` or `disabled`.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
    /// How long a provisional tenant whose database holds no information is
    /// kept before it is retired, in human-readable form (`"10m"`).
    ///
    /// A creating operation registers a tenant it targets for the first time
    /// and prepares its database before the operation runs; until that
    /// database holds information the tenant is answered as nonexistent
    /// (clause 5.5.10). A refused creating operation leaves such a tenant
    /// behind, and its database and row are removed once it has not been
    /// provisioned for this long. Longer than `request_limits.timeout`, the
    /// longest a creating request can take, or a request still running could
    /// find its database gone.
    #[toml_comment(inline)]
    pub provisional_tenant_lifetime: HumanDuration,
    /// Where each named tenant's data lives.
    pub databases: TenantDatabasesConfig,
    /// How many tenant runtimes stay open, and for how long.
    pub residency: ResidencyConfig,
    /// How a tenant runtime is opened on its first request.
    pub build: TenantBuildConfig,
}

impl Default for TenancyConfig {
    fn default() -> TenancyConfig {
        TenancyConfig {
            mode: FeatureMode::Enabled,
            provisional_tenant_lifetime: HumanDuration::from_secs(600),
            databases: TenantDatabasesConfig::default(),
            residency: ResidencyConfig::default(),
            build: TenantBuildConfig::default(),
        }
    }
}

impl TenancyConfig {
    /// Whether named tenants are served.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self.mode, FeatureMode::Enabled)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::tenancy::{TenancyConfig, TenantBuildConfig};

    #[test]
    fn named_tenants_are_served_by_default() {
        assert!(TenancyConfig::default().is_enabled());
    }

    #[test]
    fn a_zero_count_is_refused_as_it_is_read() {
        let refused = toml::from_str::<TenantBuildConfig>("max_concurrent = 0\nadmission_timeout = \"20s\"\n");
        assert!(refused.is_err());
    }
}
