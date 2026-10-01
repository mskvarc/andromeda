//! Batch upsert (clause 5.6.8), in both of its modes.
//!
//! Both modes are read-modify-writes of each entity (see
//! [`crate::batch::chunk_read_modify_write`]): a write against an entity read
//! holds only while the entity is unchanged since the read, and an insert of an
//! entity read as absent holds only while it still is. An entity created
//! concurrently is therefore updated or replaced by the next attempt, as clause
//! 5.6.8.4 does for an Entity that exists, rather than answered as processed
//! with its content dropped.
//!
//! Replace mode overwrites `entity_documents.payload` wholesale, so any attribute the
//! replacement omits is deleted and needs a tombstone. That diff is only
//! meaningful against the payload the entity actually had when the write
//! landed, which is what the compare-and-set guarantees. The diff is taken
//! against `entity_documents.payload` rather than the temporal catalogue
//! on purpose: catalogue rows outlive the attributes they describe, because a
//! deletion writes a tombstone rather than dropping the row, so diffing against
//! them would re-tombstone already-deleted attributes on every upsert.
//!
//! Update mode merges instead of replacing, so only explicit null-sentinel
//! removals imply tombstones. Clause 5.6.8.4 is what puts them there: its
//! update mode runs "the behaviour defined by clause 5.6.2 [...] limited to a
//! local operation", and clause 5.6.2.4 is one of the three clauses that say
//! "NGSI-LD Nulls should be supported by this operation".
//!
//! Neither mode sees an identifier twice: clause 5.5.11.2 requires the
//! instances an array repeats to be replaced or updated "in chronological
//! order", so the coordinator hands each one to a separate round, and a chunk
//! carries at most one instance of any entity.

use crate::{
    batch::{BatchResult, chunk::ChunkWrite},
    entity_invariant::EntityInvariant,
    error::EntityError,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::Value;
use std::sync::Arc;

/// Replace mode.
mod replace;
/// Update mode.
mod update;

/// One chunk's per-entity upsert results.
type UpsertOutcome = (BatchResult, Vec<Identifier>);

/// One chunk's per-entity upsert results, the failures it collected while
/// preparing them, and the identifiers it created.
pub(super) type UpsertChunkOutcome = (BatchResult, Vec<(Identifier, EntityError)>, Vec<Identifier>);

/// The payload `write` stores, shared rather than copied.
///
/// # Errors
///
/// Reports [`EntityInvariant::WriteWithoutStoredState`] for a write that stores
/// no payload, which no insert or update is.
fn stored_payload(write: &ChunkWrite) -> Result<Arc<Value>, Box<(Identifier, EntityError)>> {
    match write.entity.record() {
        Some(record) => Ok(Arc::clone(&record.payload)), // clone: Arc ref-count bump
        None => Err(Box::new((
            write.id.clone(), // clone: the report owns the identifier it names
            EntityInvariant::WriteWithoutStoredState {
                entity: write.id.clone(), // clone: the invariant owns the entity it names
            }
            .into(),
        ))),
    }
}
