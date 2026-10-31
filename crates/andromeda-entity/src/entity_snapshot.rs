//! One entity's state together with the version a read-modify-write compares
//! against.

use andromeda_context_commit::entity_version::EntityVersion;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use std::sync::Arc;

/// One entity's state together with the version a
/// `Precondition::UnchangedSince` compares against.
///
/// A read-modify-write reads this, computes its result, and states the
/// version it assumed; the store refuses the write if the entity has moved on
/// since. That is what replaces reading inside the writing transaction.
#[derive(Debug)]
pub struct EntitySnapshot {
    /// The entity as it stood when it was read, shared by the computation and
    /// every notification the write plans from it.
    pub state: Arc<EntityState>,
    /// The version the entity stood at in the same read.
    pub version: EntityVersion,
    /// The entity's clause 4.8 system attributes in the same read: the
    /// `createdAt` a write computed from this read keeps, and what a deletion
    /// notifies once the row is gone.
    pub sys_attrs: SysAttrsSnapshot,
}
