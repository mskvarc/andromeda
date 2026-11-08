//! The whole state a batch write stores for one entity, kept across the
//! attempts of that write.
//!
//! A create, or a replacement, converts the request's document into the state
//! it stores once. A write the store refuses is rebuilt from the state an
//! earlier attempt stored rather than from the request again: only the entry
//! holds that state once the refused write is dropped, so a later attempt
//! takes it back without copying it, and stamps its system times again.

use crate::{
    batch::{chunk::ChunkWrite, chunk_read_modify_write::PlannedWrite, prepare::PreparedEntity},
    entity_invariant::EntityInvariant,
    error::EntityError,
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::sync::Arc;

/// What the state one entity's write stores is built from on each attempt.
pub(super) enum RequestedState {
    /// The request's prepared document, which no attempt has spent yet.
    Requested(Box<PreparedEntity>),
    /// The state an earlier attempt stored.
    Built {
        /// The entity the state is for.
        id: Identifier,
        /// The state as the earlier attempt stored it.
        state: Arc<EntityState>,
    },
}

impl RequestedState {
    /// The entity the state is for.
    pub(super) fn id(&self) -> &Identifier {
        match self {
            RequestedState::Requested(prepared) => &prepared.id,
            RequestedState::Built { id, .. } => id,
        }
    }

    /// The entity and the state this attempt stores, not yet stamped.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the refusal of a document no
    /// entity can hold, boxed because a failure is the rare outcome.
    pub(super) fn into_state(self) -> Result<(Identifier, EntityState), Box<(Identifier, EntityError)>> {
        match self {
            RequestedState::Requested(prepared) => prepared.into_state(),
            RequestedState::Built { id, state } => Ok((id, Arc::unwrap_or_clone(state))),
        }
    }
}

/// The planned write storing `write`, kept so a refused attempt rebuilds from
/// the state it stored.
///
/// # Errors
///
/// Reports the invariant when the write stores no state.
pub(super) fn built(write: ChunkWrite) -> Result<PlannedWrite<RequestedState>, Box<(Identifier, EntityError)>> {
    Ok(PlannedWrite {
        entry: RequestedState::Built {
            id: write.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            state: stored_state(&write)?,
        },
        write: Some(write),
    })
}

/// The entity state `write` stores, shared rather than copied.
///
/// # Errors
///
/// Reports [`EntityInvariant::WriteWithoutStoredState`] for a write that stores
/// no state, which no insert or update is.
pub(super) fn stored_state(write: &ChunkWrite) -> Result<Arc<EntityState>, Box<(Identifier, EntityError)>> {
    match write.entity.record() {
        Some(record) => Ok(Arc::clone(&record.state)),
        None => Err(Box::new((
            write.id.clone(), // clone: the report owns the identifier it names
            EntityInvariant::WriteWithoutStoredState {
                entity: write.id.clone(), // clone: the invariant owns the entity it names
            }
            .into(),
        ))),
    }
}
