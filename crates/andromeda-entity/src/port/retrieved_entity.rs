//! One entity read for a clause 5.6.3 retrieval, together with the clause 4.8
//! system times a `sysAttrs` response writes beside it.
//!
//! The state and the entity-level `createdAt`, `modifiedAt` and `expiresAt`
//! live on the same stored row, so a store answers both in the one read
//! instead of a retrieval asking for the times in a second one.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use std::sync::Arc;

/// One stored entity and its entity-level system times, read together.
#[derive(Clone, Debug)]
pub struct RetrievedEntity {
    /// The entity as it is stored.
    pub state: Arc<EntityState>,
    /// The entity's clause 4.8 `createdAt` and `modifiedAt`, and its clause
    /// 4.22 `expiresAt`, as the store recorded them.
    pub sys_attrs: SysAttrsSnapshot,
}
