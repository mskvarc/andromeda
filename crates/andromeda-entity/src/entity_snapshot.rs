//! One entity's state together with the version token a read-modify-write
//! compares against.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// One entity's state together with the version token a
/// `Precondition::UnchangedSince` compares against.
///
/// A read-modify-write reads this, computes its result, and states the
/// `modified_at` it assumed; the store refuses the write if the entity has
/// moved on since. That is what replaces reading inside the writing
/// transaction.
#[derive(Debug)]
pub struct EntitySnapshot {
    /// The entity as it stood when it was read, shared by the computation and
    /// every notification the write plans from it.
    pub state: Arc<EntityState>,
    /// The entity's clause 4.8 `modifiedAt` at the same instant.
    pub modified_at: DateTime<Utc>,
}
