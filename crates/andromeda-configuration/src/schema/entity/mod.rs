//! The `[entity]` tables: how entity batches run, how their deferred side
//! effects are queued, and how the spatial index is kept.

pub mod batch;
pub mod side_effects;
pub mod spatial_index;

use crate::schema::entity::{batch::EntityBatchConfig, side_effects::SideEffectsConfig, spatial_index::SpatialIndexConfig};
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// How entity batches run, how their deferred side effects are queued, and
/// how the spatial index is kept.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct EntityConfig {
    /// How a batch operation is cut into chunks and run.
    pub batch: EntityBatchConfig,
    /// The durable queue of a committed batch's deferred side effects.
    pub side_effects: SideEffectsConfig,
    /// How the resident spatial index is kept consistent with the store.
    pub spatial_index: SpatialIndexConfig,
}
