//! Batch entity creation (clause 5.6.7).
//!
//! Each entity is created as Create Entity creates it (clause 5.6.7.4), with
//! nothing read first: a create holds only while the entity does not exist,
//! so a concurrent create answers 409 for that entity alone. An identifier
//! whose identity row holds no current state (a Temporal Evolution, or a
//! deletion whose history is still queued) is created anew on that row
//! (clause 5.6.1), and the store refuses the creation when the row holds
//! an instant later than the batch's, a deletion's that committed while the
//! batch ran: the instances would be recorded before the tombstones that end
//! their predecessors. Such an entity is created again after that instant.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        admitted_batch::{AdmittedBatchEntity, prepared_chunk},
        chunk_read_modify_write::{ChunkRead, ChunkRewrite, PlannedWrite},
        requested_state::{RequestedState, built},
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use std::collections::HashSet;

/// Creating each entity of a chunk (clause 5.6.7.4 runs the behaviour of
/// clause 5.6.1 per entity).
struct CreateRewrite<'a> {
    /// The service whose insert builds each write.
    service: &'a EntityService,
    /// The branch the chunk writes.
    target: &'a ContextTarget,
}

impl ChunkRewrite for CreateRewrite<'_> {
    type Entry = RequestedState;

    const READ: ChunkRead = ChunkRead::Nothing;

    fn id(entry: &RequestedState) -> &Identifier {
        entry.id()
    }

    fn build(
        &self,
        entry: RequestedState,
        _snapshot: Option<&EntitySnapshot>,
        now: SystemInstant,
    ) -> Result<PlannedWrite<RequestedState>, Box<(Identifier, EntityError)>> {
        let (id, state) = entry.into_state()?;
        built(self.service.chunk_insert(self.target, id, state, now)?)
    }
}

impl EntityService {
    /// Reject the second and later appearances of a repeated identifier.
    ///
    /// A batch that names one entity twice is asking to create it twice, and
    /// the second is a conflict exactly as it would be in a separate request
    /// (NGSI-LD 6.4.3.1).
    pub(super) fn dedupe_create_entities(entities: Vec<AdmittedBatchEntity>) -> (Vec<AdmittedBatchEntity>, Vec<(Identifier, EntityError)>) {
        let first_instances = {
            let mut seen = HashSet::with_capacity(entities.len());
            entities.iter().map(|entity| seen.insert(entity.id())).collect::<Vec<_>>()
        };
        let mut deduped = Vec::with_capacity(entities.len());
        let mut errors = Vec::new();
        for (entity, first) in entities.into_iter().zip(first_instances) {
            if first {
                deduped.push(entity);
            } else {
                errors.push((entity.id().clone(), EntityError::already_exists(entity.id()))); // clone: error result owns the entity ID
            }
        }

        (deduped, errors)
    }

    /// Create the entities of one chunk at `now`, answering an identifier the chunk
    /// repeats with a conflict. Fails only on a store failure that concerns the whole chunk.
    pub(super) async fn batch_create_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        now: SystemInstant,
    ) -> Result<BatchResult, EntityError> {
        // Clause 5.6.7.4 runs the behaviour of Create Entity for each item, and
        // clause 5.5.4 excepts only the fragments of a partial update or a
        // merge from its NGSI-LD Null prohibitions.
        let mut errors = Vec::new();
        let mut seen: HashSet<Identifier> = HashSet::with_capacity(chunk.len());
        let mut entries = Vec::with_capacity(chunk.len());
        for entity in prepared_chunk(chunk) {
            // clone: the set remembers the identifier the entity moves on with
            if seen.insert(entity.id.clone()) {
                entries.push(RequestedState::Requested(Box::new(entity)));
            } else {
                errors.push((entity.id.clone(), EntityError::already_exists(&entity.id))); // clone: error result owns the entity ID
            }
        }

        let rewrite = CreateRewrite {
            service: self,
            target: placement.target(),
        };
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, entries, now).await?;
        errors.extend(outcome.errors);

        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
