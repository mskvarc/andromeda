//! The `[temporal]` tables: reading, recording and storing Temporal
//! Evolutions.

pub mod core_write_history;
pub mod history;
pub mod query;
pub mod storage;

use crate::schema::temporal::{history::TemporalHistoryConfig, query::TemporalQueryConfig, storage::TemporalStorageConfig};
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// How Temporal Evolutions are read, recorded and stored.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemporalConfig {
    /// How much of a Temporal Evolution one read returns.
    pub query: TemporalQueryConfig,
    /// What is recorded, and how the recording queue is drained.
    pub history: TemporalHistoryConfig,
    /// How history is partitioned, converted and aged out.
    pub storage: TemporalStorageConfig,
}
