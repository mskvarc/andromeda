//! The `[entity]` tables: how entity batches run, how their deferred side
//! effects are queued, and how the spatial index is kept.

pub mod batch;
pub mod side_effects;
pub mod spatial_index;

use crate::schema::entity::{batch::EntityBatchConfig, side_effects::SideEffectsConfig, spatial_index::SpatialIndexConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How entity batches run, how their deferred side effects are queued, and
/// how the spatial index is kept.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Default)]
#[schemars(description = "How entity batches run, how their deferred side effects are queued, and how the spatial index is kept.")]
#[schemars(deny_unknown_fields)]
pub struct EntityConfig {
    /// How a batch operation is cut into chunks and run.
    #[schemars(description = "How a batch operation is cut into chunks and run.")]
    pub batch: EntityBatchConfig,
    /// The durable queue of a committed batch's deferred side effects.
    #[schemars(description = "The durable queue of change events a `Prefer: respond-async` batch leaves for a worker.")]
    pub side_effects: SideEffectsConfig,
    /// How the resident spatial index is kept consistent with the store.
    #[schemars(description = "How each tenant's in-memory spatial index stays consistent with the database.")]
    pub spatial_index: SpatialIndexConfig,
}
