//! The `[temporal]` tables: reading, recording and storing Temporal
//! Evolutions.

pub mod core_write_history;
pub mod history;
pub mod query;
pub mod storage;

use crate::schema::temporal::{history::TemporalHistoryConfig, query::TemporalQueryConfig, storage::TemporalStorageConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How Temporal Evolutions are read, recorded and stored.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "How the broker reads, records and stores the Temporal Evolution of entities.")]
pub struct TemporalConfig {
    /// How much of a Temporal Evolution one read returns.
    #[schemars(description = "How much of a Temporal Evolution one temporal read returns before it is paged.")]
    pub query: TemporalQueryConfig,
    /// What is recorded, and how the recording queue is drained.
    #[schemars(description = "Whether Core API writes are recorded in history, and how the queue that records history is drained.")]
    pub history: TemporalHistoryConfig,
    /// How history is partitioned, converted and aged out.
    #[schemars(description = "How history is partitioned, converted to the columnstore and aged out.")]
    pub storage: TemporalStorageConfig,
}
