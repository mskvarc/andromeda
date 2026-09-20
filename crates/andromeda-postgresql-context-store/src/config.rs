//! PostgreSQL connection and pool configuration.

use crate::maintenance_config::MaintenanceConfig;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;
use url::Url;
use url_macro::url;

/// Invalid PostgreSQL pool or maintenance configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DatabaseConfigError {
    /// The pool requires at least one connection.
    #[error("database.max_connections must be at least 1")]
    ZeroMaxConnections,
    /// Context-hit persistence requires a positive cadence.
    #[error("database.context_hit_flush_secs must be at least 1")]
    ZeroContextHitFlushCadence,
    /// The connection acquire deadline requires a positive number of seconds.
    #[error("database.acquire_timeout_secs must be at least 1")]
    ZeroAcquireTimeout,
}

/// A transaction-pooling proxy (pgbouncer) fronting PostgreSQL on the request path.
///
/// Its presence in [`DatabaseConfig`] is the single switch between a normal
/// deployment and a pgbouncer deployment: omit the whole `[database.pooler]`
/// section for a broker that connects straight to PostgreSQL, add it to route
/// the request path through the proxy while provisioning, migration and
/// maintenance keep using [`DatabaseConfig::url`] directly.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct PoolerConfig {
    /// The transaction pooler's PostgreSQL-protocol URL, used by the request
    /// path in place of [`DatabaseConfig::url`]. The pooler forwards to the same
    /// PostgreSQL [`DatabaseConfig::url`] names, so credentials and database are
    /// normally identical and only the host and port differ.
    #[toml_comment(inline)]
    pub url: Url,
}

impl Default for PoolerConfig {
    // Manual because `url::Url` has no `Default`, yet `toml_comment::TomlComment`
    // requires one to render the config template. The placeholder is pgbouncer's
    // default port on localhost; it is only ever the commented example, never a
    // live default, since the whole `[database.pooler]` section is absent unless
    // an operator writes it.
    fn default() -> PoolerConfig {
        PoolerConfig {
            url: url!("postgres://postgres:postgres@localhost:6432/postgres"),
        }
    }
}

/// Postgres `synchronous_commit` durability, applied to each tenant database and
/// the platform database with `ALTER DATABASE ... SET` rather than per connection.
///
/// A database-level default is inherited by every future session, so it survives
/// a transaction-pooling proxy that binds a different backend to each transaction;
/// a per-connection `SET` would instead be carried by a pooled backend to the
/// next client that borrows it.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum SynchronousCommit {
    /// Wait for local WAL flush.
    On,
    /// Acknowledge before WAL flush.
    #[default]
    Off,
    /// Wait for local WAL flush without synchronous standbys.
    Local,
    /// Wait for a synchronous standby to apply WAL.
    RemoteApply,
    /// Wait for a synchronous standby to write WAL.
    RemoteWrite,
}

impl SynchronousCommit {
    /// Render the literal accepted by PostgreSQL's `SET` and `ALTER DATABASE ...
    /// SET` statements.
    #[must_use]
    pub const fn as_postgres_setting(self) -> &'static str {
        match self {
            SynchronousCommit::On => "on",
            SynchronousCommit::Off => "off",
            SynchronousCommit::Local => "local",
            SynchronousCommit::RemoteApply => "remote_apply",
            SynchronousCommit::RemoteWrite => "remote_write",
        }
    }
}

/// PostgreSQL pool configuration.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct DatabaseConfig {
    /// Validated PostgreSQL connection URL. This always names PostgreSQL itself,
    /// never a proxy: provisioning, migration and maintenance connect here, and
    /// so does the request path unless a [`pooler`](Self::pooler) is configured.
    #[toml_comment(inline)]
    pub url: Url,
    /// An optional transaction-pooling proxy for the request path. Present means
    /// a pgbouncer deployment (the request path uses the pooler, everything that
    /// must keep session state across transactions still uses [`url`](Self::url));
    /// absent means a normal deployment where every connection uses
    /// [`url`](Self::url).
    #[serde(default)]
    pub pooler: Option<PoolerConfig>,
    /// Maximum connections in the main pool.
    pub max_connections: u32,
    /// How long a request-path connection checkout waits for a free connection
    /// before it is shed as backpressure. Kept below `http.request_timeout_secs`
    /// so a saturated pool surfaces as a 503 with `Retry-After` before the HTTP
    /// request deadline races it, rather than as a 500 after a long stall.
    pub acquire_timeout_secs: u64,
    /// Commit durability installed on each database with `ALTER DATABASE ... SET`.
    #[toml_comment(inline)]
    pub synchronous_commit: SynchronousCommit,
    /// Populate the attribute-name and entity-type-name caches at tenant
    /// startup.
    pub preload_name_dictionaries: bool,
    /// Interval between JSON-LD hit-counter flushes.
    pub context_hit_flush_secs: u64,
    /// Adaptive PostgreSQL inspection and maintenance policy.
    pub maintenance: MaintenanceConfig,
}

impl Default for DatabaseConfig {
    fn default() -> DatabaseConfig {
        DatabaseConfig {
            // Compile-time validated, so the built-in default carries no runtime parse or panic.
            url: url!("postgres://postgres:postgres@localhost:5432/postgres"),
            // No proxy by default: the request path and the direct path are the same.
            pooler: None,
            max_connections: 32,
            acquire_timeout_secs: 5,
            synchronous_commit: SynchronousCommit::Off,
            preload_name_dictionaries: true,
            context_hit_flush_secs: 5,
            maintenance: MaintenanceConfig::default(),
        }
    }
}

impl DatabaseConfig {
    /// Validate PostgreSQL pool and maintenance limits.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseConfigError::ZeroMaxConnections`] when the pool is
    /// configured without a single connection,
    /// [`DatabaseConfigError::ZeroContextHitFlushCadence`] when the JSON-LD
    /// hit-counter flush interval is zero, and
    /// [`DatabaseConfigError::ZeroAcquireTimeout`] when the connection acquire
    /// deadline is zero.
    pub const fn validate(&self) -> Result<(), DatabaseConfigError> {
        if self.max_connections == 0 {
            return Err(DatabaseConfigError::ZeroMaxConnections);
        }
        if self.context_hit_flush_secs == 0 {
            return Err(DatabaseConfigError::ZeroContextHitFlushCadence);
        }
        if self.acquire_timeout_secs == 0 {
            return Err(DatabaseConfigError::ZeroAcquireTimeout);
        }
        Ok(())
    }

    /// The request-path connection acquire deadline as a [`Duration`].
    #[must_use]
    pub const fn acquire_timeout(&self) -> Duration {
        Duration::from_secs(self.acquire_timeout_secs)
    }

    /// The base URL the request path connects through: the
    /// [`pooler`](Self::pooler) when one is configured, otherwise
    /// [`url`](Self::url). Per-tenant serving pools and the platform serving pool
    /// are derived from this.
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
    /// lock bypass any [`pooler`](Self::pooler) and reach PostgreSQL directly.
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
}

#[cfg(test)]
mod tests {
    use super::{DatabaseConfig, DatabaseConfigError, PoolerConfig, SynchronousCommit};
    use crate::maintenance_config::MaintenanceConfig;
    use std::str::FromStr;

    #[test]
    fn the_built_in_default_database_url_is_valid() {
        pretty_assertions::assert_eq!(DatabaseConfig::default().url.as_str(), "postgres://postgres:postgres@localhost:5432/postgres");
    }

    #[test]
    fn synchronous_commit_modes_parse_and_render_the_postgresql_setting() {
        for (raw, expected, setting) in [
            ("on", SynchronousCommit::On, "on"),
            ("off", SynchronousCommit::Off, "off"),
            ("local", SynchronousCommit::Local, "local"),
            ("remote-apply", SynchronousCommit::RemoteApply, "remote_apply"),
            ("remote-write", SynchronousCommit::RemoteWrite, "remote_write"),
        ] {
            let parsed = SynchronousCommit::from_str(raw).expect("known synchronous commit mode");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_postgres_setting(), setting);
        }
        assert!(SynchronousCommit::from_str("enabled").is_err());
        assert_eq!(SynchronousCommit::default(), SynchronousCommit::Off);
    }

    #[test]
    fn zero_context_hit_flush_cadence_is_rejected() {
        let config = DatabaseConfig {
            url: "postgres://localhost/postgres".parse().expect("database URL"),
            pooler: None,
            max_connections: 1,
            acquire_timeout_secs: 5,
            synchronous_commit: SynchronousCommit::Off,
            preload_name_dictionaries: true,
            context_hit_flush_secs: 0,
            maintenance: MaintenanceConfig::default(),
        };
        assert_eq!(config.validate(), Err(DatabaseConfigError::ZeroContextHitFlushCadence));
    }

    #[test]
    fn a_zero_acquire_timeout_is_rejected() {
        let config = DatabaseConfig {
            acquire_timeout_secs: 0,
            ..DatabaseConfig::default()
        };
        assert_eq!(config.validate(), Err(DatabaseConfigError::ZeroAcquireTimeout));
    }

    #[test]
    fn the_default_acquire_timeout_is_five_seconds() {
        use std::time::Duration;
        assert_eq!(DatabaseConfig::default().acquire_timeout(), Duration::from_secs(5));
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
            }),
            ..DatabaseConfig::default()
        };
        assert!(config.is_pooled());
        pretty_assertions::assert_eq!(config.request_url().as_str(), "postgres://postgres:postgres@pgbouncer:6432/postgres");
        pretty_assertions::assert_eq!(config.direct_url().as_str(), "postgres://postgres:postgres@postgres:5432/postgres");
    }
}
