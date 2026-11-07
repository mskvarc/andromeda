//! The `[database]` table: the PostgreSQL the broker stores in, and how it
//! connects.

pub mod connection;
pub mod maintenance;
pub mod name_cache;
pub mod pooler;
pub mod write_reserve;

use crate::{
    human_duration::HumanDuration,
    schema::database::{
        connection::PostgresConnection,
        maintenance::MaintenanceConfig,
        name_cache::NameCacheConfig,
        pooler::PoolerConfig,
        write_reserve::WriteReserve,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use strum::{Display, EnumString};
use url::Url;

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
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[schemars(description = "The PostgreSQL `synchronous_commit` level the broker sets on every database it serves.")]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum SynchronousCommit {
    /// Wait for local WAL flush.
    #[default]
    #[schemars(description = "Acknowledge a commit once its WAL is flushed to disk, and on synchronous standbys when any are configured.")]
    On,
    /// Acknowledge before WAL flush.
    #[schemars(
        description = "Acknowledge a commit before its WAL is flushed. Ingest is faster, but a server crash loses the transactions acknowledged in the last three times `wal_writer_delay`, and notifications for them may already have gone out."
    )]
    Off,
    /// Wait for local WAL flush without synchronous standbys.
    #[schemars(description = "Wait for the local WAL flush only, never for synchronous standbys.")]
    Local,
    /// Wait for a synchronous standby to apply WAL.
    #[schemars(description = "Wait until synchronous standbys have applied the WAL, so a read on a standby sees the commit.")]
    RemoteApply,
    /// Wait for a synchronous standby to write WAL.
    #[schemars(description = "Wait until synchronous standbys have handed the WAL to their operating system, without waiting for it to reach disk.")]
    RemoteWrite,
}

/// The PostgreSQL the broker stores in, and how it connects.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone)]
#[schemars(description = "The PostgreSQL server the broker stores in, and how it connects to it.")]
#[schemars(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// The PostgreSQL server and the default tenant's database. It always
    /// names PostgreSQL itself, never a proxy: provisioning, migration and
    /// maintenance connect here, and so does the request path unless
    /// `[database.pooler]` is configured. It has no default.
    #[schemars(
        description = "The PostgreSQL server and the default tenant's database, as a connection URL or as a table of its parts. It has no default: set it here or in `ANDROMEDA__DATABASE__CONNECTION`, or each part in its own variable such as `ANDROMEDA__DATABASE__CONNECTION__PASSWORD`. It always names PostgreSQL itself, never a proxy: tenant creation, migrations and maintenance connect here even when `database.pooler` is set, and so do requests when it is not."
    )]
    pub connection: PostgresConnection,
    /// The most connections the platform pool opens: the pool over the
    /// platform database that holds the tenant registry, the wake schedule and
    /// the forwarded-subscription routes. Always direct to PostgreSQL.
    #[schemars(
        description = "The most connections the platform pool opens, in each broker process. It reads and writes the tenant registry every tenant load consults, the schedule of tenants with due work and the routes of forwarded subscriptions, and always connects to `database.connection`, even when `database.pooler` is set. Tenants size their own pools with `tenancy.default_tenant.max_connections` and `tenancy.databases.max_connections`."
    )]
    pub platform_max_connections: NonZeroU32,
    /// Percentage of each tenant pool that read requests (queries, retrievals,
    /// discovery and temporal reads) never hold, so writes keep connections
    /// however many reads queue. Rounded down: a pool too small to spare a
    /// whole connection keeps every one for reads. Below 100.
    #[schemars(
        description = "The percentage of each tenant pool that read requests (queries, retrievals, discovery and temporal reads) never hold, so writes always find a connection however many reads queue. Rounded down to whole connections: a pool too small to spare one keeps every connection for reads."
    )]
    pub write_reserve_percent: WriteReserve,
    /// How long a request-path connection checkout waits for a free connection
    /// before it is shed as backpressure, in human-readable form (`"5s"`). Kept
    /// below `request_limits.timeout` so a saturated pool surfaces as a 503
    /// with `Retry-After` before the request deadline races it.
    #[schemars(description = "How long a request waits for a free database connection before it is answered `503` with `Retry-After`.")]
    pub acquire_timeout: HumanDuration,
    /// Commit durability installed on each database with `ALTER DATABASE ...
    /// SET`: `on` by default; `off` trades the durability of acknowledged
    /// writes for ingest throughput.
    #[schemars(
        description = "The PostgreSQL `synchronous_commit` level set on the default tenant's database and every tenant database. It is installed with `ALTER DATABASE ... SET`, so every session inherits it, including sessions a transaction-pooling proxy hands out. With `\"off\"`, a server crash can lose writes clients were already told succeeded."
    )]
    pub synchronous_commit: SynchronousCommit,
    /// A transaction-pooling proxy on the request path. Left out, every
    /// connection goes to `connection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "A transaction-pooling proxy, such as PgBouncer in transaction mode, on the request path. Left out, every connection goes straight to `database.connection`."
    )]
    pub pooler: Option<PoolerConfig>,
    /// The tenant name dictionary caches.
    #[schemars(description = "The in-memory caches of each tenant's attribute and entity type names.")]
    pub name_cache: NameCacheConfig,
    /// Adaptive PostgreSQL inspection and maintenance.
    #[schemars(description = "The adaptive PostgreSQL inspection and maintenance controller.")]
    pub maintenance: MaintenanceConfig,
}

/// Every `[database]` key that has a built-in default, at it: all but the
/// connection, which has none, and the pooler, which is left out.
#[derive(Serialize, Debug, Clone, Copy)]
pub struct DatabaseDefaults {
    /// See [`DatabaseConfig::platform_max_connections`].
    pub platform_max_connections: NonZeroU32,
    /// See [`DatabaseConfig::write_reserve_percent`].
    pub write_reserve_percent: WriteReserve,
    /// See [`DatabaseConfig::acquire_timeout`].
    pub acquire_timeout: HumanDuration,
    /// See [`DatabaseConfig::synchronous_commit`].
    pub synchronous_commit: SynchronousCommit,
    /// See [`DatabaseConfig::name_cache`].
    pub name_cache: NameCacheConfig,
    /// See [`DatabaseConfig::maintenance`].
    pub maintenance: MaintenanceConfig,
}

impl Default for DatabaseDefaults {
    // Manual because `NonZeroU32` has no `Default`.
    fn default() -> DatabaseDefaults {
        DatabaseDefaults {
            platform_max_connections: NonZeroU32::MIN.saturating_add(31),
            write_reserve_percent: WriteReserve::DEFAULT,
            acquire_timeout: HumanDuration::from_secs(5),
            synchronous_commit: SynchronousCommit::On,
            name_cache: NameCacheConfig::default(),
            maintenance: MaintenanceConfig::default(),
        }
    }
}

impl DatabaseConfig {
    /// The `[database]` table that connects to `connection` directly, every
    /// other key at `defaults`.
    #[must_use]
    pub const fn new(connection: PostgresConnection, defaults: DatabaseDefaults) -> DatabaseConfig {
        let DatabaseDefaults {
            platform_max_connections,
            write_reserve_percent,
            acquire_timeout,
            synchronous_commit,
            name_cache,
            maintenance,
        } = defaults;
        DatabaseConfig {
            connection,
            platform_max_connections,
            write_reserve_percent,
            acquire_timeout,
            synchronous_commit,
            pooler: None,
            name_cache,
            maintenance,
        }
    }

    /// The request-path connection acquire deadline as a [`Duration`].
    #[must_use]
    pub const fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout.duration()
    }

    /// The base URL the request path connects through: the pooler when one is
    /// configured, otherwise [`connection`](Self::connection). Per-tenant
    /// serving pools and the platform serving pool are derived from this.
    #[must_use]
    pub fn request_url(&self) -> &Url {
        self.pooler.as_ref().map_or(self.connection.url(), |pooler| pooler.connection.url())
    }

    /// The straight-to-PostgreSQL base URL for work that must keep session state
    /// across transactions.
    ///
    /// Always [`connection`](Self::connection): tenant provisioning (`CREATE DATABASE`), schema
    /// migration (the migrator holds a session advisory lock), the
    /// `ALTER DATABASE` durability settings and maintenance's session advisory
    /// lock bypass any pooler and reach PostgreSQL directly.
    #[must_use]
    pub const fn direct_url(&self) -> &Url {
        self.connection.url()
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
mod tests {
    use crate::schema::database::{DatabaseConfig, DatabaseDefaults, SynchronousCommit, connection::PostgresConnection, pooler::PoolerConfig};
    use std::{num::NonZeroU32, str::FromStr, time::Duration};
    use url_macro::url;

    fn direct() -> DatabaseConfig {
        DatabaseConfig::new(
            PostgresConnection::new(url!("postgres://postgres:postgres@postgres:5432/postgres")),
            DatabaseDefaults::default(),
        )
    }

    fn pooled(max_connections_per_tenant: NonZeroU32) -> DatabaseConfig {
        DatabaseConfig {
            pooler: Some(PoolerConfig {
                connection: PostgresConnection::new(url!("postgres://postgres:postgres@pgbouncer:6432/postgres")),
                max_connections_per_tenant,
            }),
            ..direct()
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
        let refused = toml::from_str::<PoolerConfig>("connection = \"postgres://localhost:6432/postgres\"\nmax_connections_per_tenant = 0\n");
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
        pretty_assertions::assert_eq!(direct().synchronous_commit, SynchronousCommit::On);
    }

    #[test]
    fn the_default_acquire_timeout_is_five_seconds() {
        pretty_assertions::assert_eq!(direct().acquire_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn without_a_pooler_the_request_and_direct_base_are_both_the_url() {
        let config = direct();
        assert!(!config.is_pooled());
        pretty_assertions::assert_eq!(config.request_url(), config.connection.url());
        pretty_assertions::assert_eq!(config.direct_url(), config.connection.url());
    }

    #[test]
    fn a_pooler_moves_only_the_request_base_and_leaves_the_direct_base_on_postgres() {
        let config = pooled(NonZeroU32::MIN);
        assert!(config.is_pooled());
        pretty_assertions::assert_eq!(config.request_url().as_str(), "postgres://postgres:postgres@pgbouncer:6432/postgres");
        pretty_assertions::assert_eq!(config.direct_url().as_str(), "postgres://postgres:postgres@postgres:5432/postgres");
    }
}
