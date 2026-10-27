//! The `[database]` table: the PostgreSQL the broker stores in, and how it
//! connects.

pub mod maintenance;
pub mod name_cache;
pub mod pooler;
pub mod write_reserve;

use crate::{
    human_duration::HumanDuration,
    schema::database::{maintenance::MaintenanceConfig, name_cache::NameCacheConfig, pooler::PoolerConfig, write_reserve::WriteReserve},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use strum::{Display, EnumString};
use toml_comment::TomlComment;
use url::Url;
use url_macro::url;

/// Postgres `synchronous_commit` durability, applied to each tenant database and
/// the platform database with `ALTER DATABASE ... SET` rather than per connection.
///
/// A database-level default is inherited by every future session, so it survives
/// a transaction-pooling proxy that binds a different backend to each transaction;
/// a per-connection `SET` would instead be carried by a pooled backend to the
/// next client that borrows it.
///
/// The default is [`On`](SynchronousCommit::On): a write is acknowledged only
/// once its WAL is flushed, so an Entity a client was told was created, and a
/// change a notification announced, survive a crash of the server.
/// [`Off`](SynchronousCommit::Off) is an operator's choice to trade that for
/// ingest throughput: a server crash then loses the transactions of up to three
/// times `wal_writer_delay` that were already acknowledged, and their
/// notifications may already have been sent.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum SynchronousCommit {
    /// Wait for local WAL flush.
    #[default]
    On,
    /// Acknowledge before WAL flush.
    Off,
    /// Wait for local WAL flush without synchronous standbys.
    Local,
    /// Wait for a synchronous standby to apply WAL.
    RemoteApply,
    /// Wait for a synchronous standby to write WAL.
    RemoteWrite,
}

/// The PostgreSQL the broker stores in, and how it connects.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL. It always names PostgreSQL itself, never a
    /// proxy: provisioning, migration and maintenance connect here, and so
    /// does the request path unless `[database.pooler]` is configured.
    #[toml_comment(inline)]
    pub url: Url,
    /// The most connections the platform pool opens.
    pub max_connections: u32,
    /// Percentage of each tenant pool that read requests (queries, retrievals,
    /// discovery and temporal reads) never hold, so writes keep connections
    /// however many reads queue. Rounded down: a pool too small to spare a
    /// whole connection keeps every one for reads. Below 100.
    #[toml_comment(inline)]
    pub write_reserve_percent: WriteReserve,
    /// How long a request-path connection checkout waits for a free connection
    /// before it is shed as backpressure, in human-readable form (`"5s"`). Kept
    /// below `request_limits.timeout` so a saturated pool surfaces as a 503
    /// with `Retry-After` before the request deadline races it.
    #[toml_comment(inline)]
    pub acquire_timeout: HumanDuration,
    /// Commit durability installed on each database with `ALTER DATABASE ...
    /// SET`: `on` by default; `off` trades the durability of acknowledged
    /// writes for ingest throughput.
    #[toml_comment(inline)]
    pub synchronous_commit: SynchronousCommit,
    /// A transaction-pooling proxy on the request path. Left out, every
    /// connection goes to `url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pooler: Option<PoolerConfig>,
    /// The tenant name dictionary caches.
    pub name_cache: NameCacheConfig,
    /// Adaptive PostgreSQL inspection and maintenance.
    pub maintenance: MaintenanceConfig,
}

impl Default for DatabaseConfig {
    fn default() -> DatabaseConfig {
        DatabaseConfig {
            // Compile-time validated, so the built-in default carries no runtime parse or panic.
            url: url!("postgres://postgres:postgres@localhost:5432/postgres"),
            max_connections: 32,
            write_reserve_percent: WriteReserve::DEFAULT,
            acquire_timeout: HumanDuration::from_secs(5),
            synchronous_commit: SynchronousCommit::On,
            pooler: None,
            name_cache: NameCacheConfig::default(),
            maintenance: MaintenanceConfig::default(),
        }
    }
}

impl DatabaseConfig {
    /// The request-path connection acquire deadline as a [`Duration`].
    #[must_use]
    pub const fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout.duration()
    }

    /// The base URL the request path connects through: the pooler when one is
    /// configured, otherwise [`url`](Self::url). Per-tenant serving pools and
    /// the platform serving pool are derived from this.
    #[must_use]
    pub fn request_url(&self) -> &Url {
        self.pooler.as_ref().map_or(&self.url, |pooler| &pooler.url)
    }

    /// The straight-to-PostgreSQL base URL for work that must keep session state
    /// across transactions.
    ///
    /// Always [`url`](Self::url): tenant provisioning (`CREATE DATABASE`), schema
    /// migration (the migrator holds a session advisory lock), the
    /// `ALTER DATABASE` durability settings and maintenance's session advisory
    /// lock bypass any pooler and reach PostgreSQL directly.
    #[must_use]
    pub const fn direct_url(&self) -> &Url {
        &self.url
    }

    /// Whether a transaction-pooling proxy fronts the request path, for the
    /// startup report and diagnostics.
    #[must_use]
    pub const fn is_pooled(&self) -> bool {
        self.pooler.is_some()
    }

    /// How many connections a named tenant's serving pool should open.
    ///
    /// When a pooler fronts the request path it multiplexes onto its own
    /// backends, so the serving pool holds only the pooler's small
    /// `max_connections_per_tenant`, keeping a working set larger than the
    /// runtime cache from churning the host's sockets. Without a pooler the
    /// serving pool is the real backend pool, so it takes `direct`, the
    /// tenancy-configured size.
    #[must_use]
    pub fn named_serving_max_connections(&self, direct: u32) -> u32 {
        self.pooler.as_ref().map_or(direct, |pooler| pooler.max_connections_per_tenant.get())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::database::{DatabaseConfig, SynchronousCommit, pooler::PoolerConfig};
    use std::{num::NonZeroU32, str::FromStr, time::Duration};

    #[test]
    fn the_built_in_default_database_url_is_valid() {
        pretty_assertions::assert_eq!(DatabaseConfig::default().url.as_str(), "postgres://postgres:postgres@localhost:5432/postgres");
    }

    fn pooled(max_connections_per_tenant: NonZeroU32) -> DatabaseConfig {
        DatabaseConfig {
            pooler: Some(PoolerConfig {
                url: "postgres://localhost:6432/postgres".parse().expect("pooler URL"),
                max_connections_per_tenant,
            }),
            ..DatabaseConfig::default()
        }
    }

    #[test]
    fn a_pooled_named_tenant_serves_on_the_small_pooler_cap_not_the_direct_size() {
        let two = NonZeroU32::MIN.saturating_add(1);
        pretty_assertions::assert_eq!(pooled(two).named_serving_max_connections(16), 2);
        let mut direct = pooled(two);
        direct.pooler = None;
        pretty_assertions::assert_eq!(direct.named_serving_max_connections(16), 16);
    }

    #[test]
    fn a_zero_pooler_connection_count_is_refused_as_it_is_read() {
        let refused = toml::from_str::<PoolerConfig>("url = \"postgres://localhost:6432/postgres\"\nmax_connections_per_tenant = 0\n");
        assert!(refused.is_err());
    }

    #[test]
    fn synchronous_commit_modes_parse_from_their_kebab_case_names() {
        for (raw, expected) in [
            ("on", SynchronousCommit::On),
            ("off", SynchronousCommit::Off),
            ("local", SynchronousCommit::Local),
            ("remote-apply", SynchronousCommit::RemoteApply),
            ("remote-write", SynchronousCommit::RemoteWrite),
        ] {
            pretty_assertions::assert_eq!(SynchronousCommit::from_str(raw).expect("known synchronous commit mode"), expected);
        }
        assert!(SynchronousCommit::from_str("enabled").is_err());
    }

    #[test]
    fn an_acknowledged_write_is_flushed_unless_an_operator_opts_out() {
        pretty_assertions::assert_eq!(DatabaseConfig::default().synchronous_commit, SynchronousCommit::On);
    }

    #[test]
    fn the_default_acquire_timeout_is_five_seconds() {
        pretty_assertions::assert_eq!(DatabaseConfig::default().acquire_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn without_a_pooler_the_request_and_direct_base_are_both_the_url() {
        let config = DatabaseConfig::default();
        assert!(!config.is_pooled());
        pretty_assertions::assert_eq!(config.request_url(), &config.url);
        pretty_assertions::assert_eq!(config.direct_url(), &config.url);
    }

    #[test]
    fn a_pooler_moves_only_the_request_base_and_leaves_the_direct_base_on_postgres() {
        let config = DatabaseConfig {
            url: "postgres://postgres:postgres@postgres:5432/postgres".parse().expect("direct URL"),
            pooler: Some(PoolerConfig {
                url: "postgres://postgres:postgres@pgbouncer:6432/postgres".parse().expect("pooler URL"),
                ..PoolerConfig::default()
            }),
            ..DatabaseConfig::default()
        };
        assert!(config.is_pooled());
        pretty_assertions::assert_eq!(config.request_url().as_str(), "postgres://postgres:postgres@pgbouncer:6432/postgres");
        pretty_assertions::assert_eq!(config.direct_url().as_str(), "postgres://postgres:postgres@postgres:5432/postgres");
    }
}
