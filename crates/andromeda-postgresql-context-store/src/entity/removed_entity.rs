//! One Entity a delete statement removed, and when it committed.

use andromeda_context_commit::{deleted_entity::DeletedEntity, store_instant::StoreInstant};

/// An Entity whose current state one delete statement removed.
#[derive(Debug)]
pub struct RemovedEntity {
    /// What the Entity held before it was removed.
    pub entity: DeletedEntity,
    /// The transaction timestamp of the statement's transaction.
    pub committed_at: StoreInstant,
}
