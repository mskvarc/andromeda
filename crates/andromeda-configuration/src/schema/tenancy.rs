//! The `[tenancy]` table: whether named tenants are served, where their data
//! lives, how many stay open, and how one is opened.

use crate::{
    human_duration::HumanDuration,
    schema::{db_name_prefix::DbNamePrefix, feature_mode::FeatureMode},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU32, NonZeroUsize};

/// Where each named tenant's data lives.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[schemars(description = "How each named tenant's database is named, and how many connections its pool opens.")]
#[schemars(deny_unknown_fields)]
pub struct TenantDatabasesConfig {
    /// The prefix of every per-tenant database name the broker provisions.
    #[schemars(
        title = "SQL identifier prefix",
        description = "The prefix every tenant database name starts with. Maintenance also uses it to recognise databases on the server that look like a tenant's but belong to none. Must match `[A-Za-z_][A-Za-z0-9_]*`, so the database names need no quoting."
    )]
    pub name_prefix: DbNamePrefix,
    /// The most connections one tenant's pool opens.
    ///
    /// With `tenancy.residency.max_resident`, which caps how many tenants
    /// stay open at once, this bounds the live connection count at
    /// `max_resident * max_connections` (plus the platform pools), so tenant
    /// churn cannot exhaust the server's connection limit.
    #[schemars(
        description = "The most connections each named tenant's pool opens, in each broker process. With `tenancy.residency.max_resident` it bounds the open connections at their product, plus the default tenant's pool. Behind a proxy, `database.pooler.max_connections_per_tenant` applies instead."
    )]
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

/// The default tenant's pool.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "The tenant a request reaches when it names none in `NGSILD-Tenant`, whose data lives in `database.url`.")]
#[schemars(deny_unknown_fields)]
pub struct DefaultTenantConfig {
    /// The most connections the default tenant's pool opens.
    #[schemars(
        description = "The most connections the default tenant's pool opens, in each broker process. Named tenants size theirs with `tenancy.databases.max_connections`."
    )]
    pub max_connections: NonZeroU32,
}

impl Default for DefaultTenantConfig {
    fn default() -> DefaultTenantConfig {
        DefaultTenantConfig {
            max_connections: NonZeroU32::MIN.saturating_add(31),
        }
    }
}

/// How many tenant runtimes stay open, and for how long.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "How many tenants a broker process keeps loaded, for how long, and how an unloaded tenant with due work is loaded again.")]
#[schemars(deny_unknown_fields)]
pub struct ResidencyConfig {
    /// How long an unused tenant runtime stays open before it is evicted, in
    /// human-readable form (`"15m"`).
    #[schemars(description = "How long an unused tenant stays loaded. Unloading a tenant closes its pool and stops its background work.")]
    pub idle_eviction_after: HumanDuration,
    /// The most tenant runtimes open at once.
    ///
    /// Enforced on every sweep and whenever a cold build adds a runtime. A
    /// runtime a request currently holds is never evicted, nor is a runtime
    /// woken for due work within `woken_lifetime`, so while every other runtime
    /// is held the cache overshoots it, trimmed back as requests complete and
    /// those lifetimes end. Room is made from the least recently used of the
    /// others, taking a runtime whose periodic notification falls due within
    /// `min_wake_interval` only once no runtime without one is left.
    #[schemars(
        description = "The most tenants loaded at once. A tenant with requests in flight is never unloaded, nor is a tenant loaded for due work within `tenancy.residency.woken_lifetime`, so the count can exceed this until those requests complete and those lifetimes end. Room is made by unloading the least recently used of the other tenants. A tenant with a periodic notification due within `tenancy.residency.min_wake_interval` is unloaded only when every other candidate has one too."
    )]
    pub max_resident: NonZeroUsize,
    /// How often idle and orphaned runtimes are swept, in human-readable form
    /// (`"30s"`).
    #[schemars(description = "How often idle tenants are unloaded and provisional tenants settled.")]
    pub sweep_interval: HumanDuration,
    /// The shortest wait before a just-evicted tenant with due work is woken
    /// again, in human-readable form (`"30s"`).
    ///
    /// When a tenant is evicted its earliest due work is probed and a wake
    /// recorded for it; a due instant sooner than this is pushed back to it,
    /// so evicting a tenant that is only a little overdue and reloading it
    /// cannot thrash. A failed probe records a retry this far out. A runtime
    /// with a periodic notification due within this interval is the last to
    /// be unloaded to make room; one unloaded all the same notifies up to this
    /// interval plus the waker's 5 s pass late.
    #[schemars(
        description = "The shortest wait before a tenant that was just unloaded is loaded again for due work, such as an expiry or a periodic notification. A due time sooner than this is pushed back to it, so a tenant that is only a little overdue is not unloaded and reloaded in a loop. A failed check for due work is retried this far out. A tenant with a periodic notification due within this interval is the last to be unloaded to make room. If it is unloaded all the same, that notification can be late by up to this interval plus 5 seconds."
    )]
    pub min_wake_interval: HumanDuration,
    /// How long a runtime woken for its due work stays open, in
    /// human-readable form (`"1m"`). For this long it is neither evicted as
    /// idle nor unloaded to make room, so the work it was woken for runs; after
    /// it, it is the first idle-eviction candidate, so it never displaces a
    /// runtime a request touched. Below `idle_eviction_after` to have that
    /// effect.
    #[schemars(
        description = "How long a tenant loaded for its due work stays loaded. For this long it is not unloaded, not even to make room for another tenant, so its due work runs. After it, such a tenant is the first to be unloaded again, yet never displaces a tenant a request used."
    )]
    pub woken_lifetime: HumanDuration,
    /// The most dormant tenants one wake pass opens, those with due work first
    /// and then those behind the server's extension versions. Woken runtimes are
    /// built through the same admission bound as request cold builds, so this
    /// keeps a wake burst from starving request-driven cold loads.
    #[schemars(
        description = "The most unloaded tenants one pass loads: for due work first, then, in what that leaves, for tenants whose extensions are behind the versions the server ships. Woken tenants share `tenancy.build.max_concurrent` with tenants loaded for requests, so this keeps a burst of wakes from crowding requests out."
    )]
    pub max_wakes_per_pass: NonZeroUsize,
    /// The most tenant runtimes torn down at once.
    ///
    /// Eviction closes a runtime's pool and stops its workers; every eviction
    /// takes a teardown slot first, so the runtimes alive at once stay within
    /// `max_resident` plus this bound. A cold build that must make room while
    /// every slot is busy waits for one within `tenancy.build.admission_timeout`.
    #[schemars(
        description = "The most tenants unloading at once. Loaded and unloading tenants together stay within `tenancy.residency.max_resident` plus this. A load that must make room while every slot is busy waits for one within `tenancy.build.admission_timeout`."
    )]
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
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "How a tenant is loaded on its first request.")]
#[schemars(deny_unknown_fields)]
pub struct TenantBuildConfig {
    /// The most tenant runtimes built at once.
    ///
    /// A cold build opens a connection pool and, on a schema that is behind or
    /// absent, runs the migrator over a direct connection. Requests past the
    /// bound wait for a build permit for at most `admission_timeout`; the wait
    /// is reported as `andromeda_tenant_build_admission_wait_seconds`.
    #[schemars(
        description = "The most tenants loading at once. Loading opens a pool and, when the tenant's schema is behind or absent, runs migrations over a direct connection. Requests past the bound wait for at most `tenancy.build.admission_timeout`."
    )]
    pub max_concurrent: NonZeroUsize,
    /// How long a request may wait to be admitted to a cold build before it is
    /// answered 503 with `Retry-After`, in human-readable form (`"20s"`).
    ///
    /// Covers every wait ahead of the build itself: another request's build of
    /// the same tenant, a build permit, and a teardown slot when the cache is
    /// full. Below `request_limits.timeout`, which it would otherwise race.
    #[schemars(
        description = "How long a request may wait for its tenant to start loading before it is answered `503` with `Retry-After`. Covers every wait ahead of the load itself: another request loading the same tenant, a free loading slot, and a free unloading slot when the cache is full."
    )]
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
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone)]
#[schemars(
    description = "Whether named tenants are served, how the default tenant's and each named tenant's pools are sized, how named tenants' databases are named, and how many stay loaded."
)]
#[schemars(deny_unknown_fields)]
pub struct TenancyConfig {
    /// Whether named tenants are served: `enabled` or `disabled`.
    #[schemars(
        description = "Whether named tenants are served. With `\"disabled\"` only the default tenant exists, and a request naming a tenant in `NGSILD-Tenant` is answered `501` `NoMultiTenantSupport`. On by default, so a client can name a tenant without any setup on the broker: the first operation that creates something in a tenant creates the tenant."
    )]
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
    #[schemars(
        description = "How long a new tenant whose database still holds nothing is kept before its database is dropped. A creating operation that names a tenant for the first time prepares its database before it runs, and the tenant is answered as nonexistent until that database holds information; a refused operation leaves such a tenant behind."
    )]
    pub provisional_tenant_lifetime: HumanDuration,
    /// The default tenant's pool.
    #[schemars(description = "The default tenant's pool.")]
    pub default_tenant: DefaultTenantConfig,
    /// Where each named tenant's data lives.
    #[schemars(description = "How each named tenant's database is named and pooled.")]
    pub databases: TenantDatabasesConfig,
    /// How many tenant runtimes stay open, and for how long.
    #[schemars(description = "How many tenants stay loaded, and for how long.")]
    pub residency: ResidencyConfig,
    /// How a tenant runtime is opened on its first request.
    #[schemars(description = "How a tenant is loaded on its first request.")]
    pub build: TenantBuildConfig,
}

impl Default for TenancyConfig {
    fn default() -> TenancyConfig {
        TenancyConfig {
            mode: FeatureMode::Enabled,
            provisional_tenant_lifetime: HumanDuration::from_secs(600),
            default_tenant: DefaultTenantConfig::default(),
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
