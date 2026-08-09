//! PostgreSQL connection configuration.

use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use url::Url;

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
#[derive(Config, Debug, Clone, Serialize)]
pub struct DatabaseConfig {
    /// Validated PostgreSQL connection URL.
    #[config(env = "ANDROMEDA_DATABASE_URL", default = "postgres://postgres:postgres@localhost:5432/postgres")]
    pub url: Url,
    /// Maximum connections in the main pool.
    #[config(env = "ANDROMEDA_DATABASE_MAX_CONNECTIONS", default = 32)]
    pub max_connections: u32,
    /// Commit durability applied to each connection.
    #[config(env = "ANDROMEDA_DATABASE_SYNCHRONOUS_COMMIT", default = "off")]
    pub synchronous_commit: SynchronousCommit,
}
