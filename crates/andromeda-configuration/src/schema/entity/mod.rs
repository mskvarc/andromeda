//! The `[entity]` tables: how entity batches run, how their deferred side
//! effects are queued, and how the spatial index is kept.

pub mod batch;
pub mod side_effects;
pub mod spatial_index;

use crate::schema::entity::{
    batch::EntityBatchConfig,
    side_effects::{SideEffectsConfig, SideEffectsConfigError},
    spatial_index::{SpatialIndexConfig, SpatialIndexConfigError},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// An unusable `[entity]` table.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EntityConfigError {
    /// The side-effect queue is unusable.
    #[error(transparent)]
    SideEffects(#[from] SideEffectsConfigError),
    /// The spatial index settings are unusable.
    #[error(transparent)]
    SpatialIndex(#[from] SpatialIndexConfigError),
}

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

impl EntityConfig {
    /// Validate the side-effect queue and the spatial index.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityConfigError`] of the first unusable value.
    pub fn validate(&self) -> Result<(), EntityConfigError> {
        self.side_effects.validate()?;
        self.spatial_index.validate()?;
        Ok(())
    }
}
