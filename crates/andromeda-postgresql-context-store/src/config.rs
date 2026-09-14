//! PostgreSQL connection and pool configuration.

use crate::maintenance_config::MaintenanceConfig;
use serde::{Deserialize, Serialize};
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
}

/// Postgres `synchronous_commit` setting applied per pool connection.
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
    /// Render the literal accepted by PostgreSQL's `SET` statement.
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
    /// Validated PostgreSQL connection URL.
    #[toml_comment(inline)]
    pub url: Url,
    /// Maximum connections in the main pool.
    pub max_connections: u32,
    /// Commit durability applied to each connection.
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
            max_connections: 32,
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
    /// configured without a single connection, and
    /// [`DatabaseConfigError::ZeroContextHitFlushCadence`] when the JSON-LD
    /// hit-counter flush interval is zero.
    pub const fn validate(&self) -> Result<(), DatabaseConfigError> {
        if self.max_connections == 0 {
            return Err(DatabaseConfigError::ZeroMaxConnections);
        }
        if self.context_hit_flush_secs == 0 {
            return Err(DatabaseConfigError::ZeroContextHitFlushCadence);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DatabaseConfig, DatabaseConfigError, SynchronousCommit};
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
            max_connections: 1,
            synchronous_commit: SynchronousCommit::Off,
            preload_name_dictionaries: true,
            context_hit_flush_secs: 0,
            maintenance: MaintenanceConfig::default(),
        };
        assert_eq!(config.validate(), Err(DatabaseConfigError::ZeroContextHitFlushCadence));
    }
}
