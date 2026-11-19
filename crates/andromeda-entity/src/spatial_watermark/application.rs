//! Taking a change read into the index on a blocking worker.
//!
//! Each application owns the lock that keeps every other writer of the index
//! out, and records the read in the watermark state itself. A query dropped by
//! its deadline while the index takes its read in therefore never leaves the
//! read half applied with the lock released, and no async runtime worker
//! replaces entries one at a time.

use crate::{
    error::EntityError,
    port::{
        blocking_executor::BlockingTask,
        spatial_snapshot::{EntityLocation, LocationRead},
    },
    spatial_placement::LocationPlacement,
    spatial_watermark::standing::WatermarkState,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::identifier_spelling::IdentifierSpelling;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use async_lock::{Mutex, MutexGuardArc};
use std::sync::Arc;

/// A bounded step's read, taken in under the watermark state it holds.
pub(super) struct StepApplication {
    /// The index the read is taken into.
    pub(super) index: Arc<SpatialEntityIndex>,
    /// Which partitions an entity's location occupies.
    pub(super) placement: LocationPlacement,
    /// The step's read.
    pub(super) read: LocationRead,
    /// The arrival count when the step's read started.
    pub(super) round: u64,
    /// The watermark state, held from before the read until the read is
    /// recorded.
    pub(super) state: MutexGuardArc<WatermarkState>,
}

impl BlockingTask for StepApplication {
    fn run(self: Box<Self>) -> Result<(), EntityError> {
        let StepApplication {
            index,
            placement,
            read,
            round,
            mut state,
        } = *self;
        take_in(&index, &placement, read.locations);
        state.caught_up(read.horizon, round);
        Ok(())
    }
}

/// The background catch-up's read of a backlog, taken in while the index is
/// behind.
///
/// No query writes the index while it is behind, so this holds the watermark
/// state only to record the read once it is in.
pub(super) struct BacklogApplication {
    /// The index the read is taken into.
    pub(super) index: Arc<SpatialEntityIndex>,
    /// Which partitions an entity's location occupies.
    pub(super) placement: LocationPlacement,
    /// The backlog's read.
    pub(super) read: LocationRead,
    /// The arrival count when the backlog's read started.
    pub(super) round: u64,
    /// The watermark state the read is recorded in.
    pub(super) state: Arc<Mutex<WatermarkState>>,
}

impl BlockingTask for BacklogApplication {
    fn run(self: Box<Self>) -> Result<(), EntityError> {
        let BacklogApplication {
            index,
            placement,
            read,
            round,
            state,
        } = *self;
        take_in(&index, &placement, read.locations);
        state.lock_blocking().caught_up(read.horizon, round);
        Ok(())
    }
}

/// Replace every read entity's entries with the partitions it now occupies.
fn take_in(index: &SpatialEntityIndex, placement: &LocationPlacement, locations: Vec<EntityLocation>) {
    for location in locations {
        let placements = placement.of(&location.entity_types, location.location.as_ref().map(Geometry::form));
        index.replace_entity(IdentifierSpelling::from(&location.entity_id), placements);
    }
}
