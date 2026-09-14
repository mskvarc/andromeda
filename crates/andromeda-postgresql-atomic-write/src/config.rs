//! PostgreSQL atomic-write execution configuration.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use toml_comment::TomlComment;

/// Bulk-write execution mode for batch entity writes.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum BulkWriteMode {
    /// Send PostgreSQL arrays through `UNNEST`.
    #[default]
    Unnest,
    /// Stream rows into a transaction-scoped staging table.
    CopyStaging,
}

/// PostgreSQL atomic-write strategy.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
pub struct AtomicWriteConfig {
    /// Database bulk-write strategy.
    #[toml_comment(inline)]
    pub bulk_mode: BulkWriteMode,
}
