//! Batch upsert in replace mode (clause 5.6.8.4): an Entity that exists is
//! replaced, one that does not is created.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        admitted_batch::{AdmittedBatchEntity, prepared_chunk},
        chunk::UpdatedStates,
        chunk_read_modify_write::{ChunkRead, ChunkRewrite, PlannedWrite},
        requested_state::{RequestedState, built},
        upsert::UpsertOutcome,
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use std::sync::Arc;

/// Replacing each entity of a chunk, or creating it when an attempt finds it
/// absent (clause 5.6.8.4, replace mode).
struct ReplaceRewrite<'a> {
    /// The service whose replacement builds each write.
    service: &'a EntityService,
    /// The branch the chunk writes.
    target: &'a ContextTarget,
}

impl ChunkRewrite for ReplaceRewrite<'_> {
    type Entry = RequestedState;

    const READ: ChunkRead = ChunkRead::Snapshots;

    fn id(entry: &RequestedState) -> &Identifier {
        entry.id()
    }

    fn build(
        &self,
        entry: RequestedState,
        snapshot: Option<&EntitySnapshot>,
        now: SystemInstant,
    ) -> Result<PlannedWrite<RequestedState>, Box<(Identifier, EntityError)>> {
        let (id, replacement) = entry.into_state()?;
        self.service.replacement_write(self.target, id, replacement, snapshot, now)
    }
}

impl EntityService {
    /// Replace each entity of `chunk` at `now`, creating the absent ones. Per-entity
    /// failures are pushed onto `errors`; the outcome lists the created identifiers
    /// apart. Fails only on a store failure that concerns the whole chunk.
    pub(in crate::batch) async fn batch_upsert_replace_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        errors: &mut Vec<(Identifier, EntityError)>,
        now: SystemInstant,
    ) -> Result<UpsertOutcome, EntityError> {
        // Clause 5.6.8.4 gives replace mode two branches and neither admits the
        // sentinel: an Entity that does not exist is created by "the behaviour
        // defined by clause 5.6.1", and one that does "shall be completely
        // replaced by the new Entity content provided", which is the Replace
        // Entity of clause 5.6.18, whose clause 5.6.18.4 states outright that
        // NGSI-LD Nulls are not supported. Clause 5.5.4 excepts only partial
        // update and merge fragments, and a replacement is neither.
        let prepared = prepared_chunk(chunk);
        let rewrite = ReplaceRewrite {
            service: self,
            target: placement.target(),
        };
        let entries = prepared.into_iter().map(|prepared| RequestedState::Requested(Box::new(prepared))).collect();
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, entries, now).await?;
        errors.extend(outcome.errors);
        Ok((
            BatchResult {
                success: outcome.success,
                errors: Vec::new(),
            },
            outcome.created,
        ))
    }

    /// The write that replaces the entity `snapshot` read with
    /// `replacement`, tombstoning whatever the replacement drops (clause
    /// 5.6.8), or creates it when the attempt found it absent.
    ///
    /// Mirrors the single replace (clause 5.6.18): one precise event for every
    /// created, updated, or deleted attribute instance, every modified
    /// instance observed, and no write at all when the replacement restates
    /// the entity exactly.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, for the batch's per-entity report.
    fn replacement_write(
        &self,
        target: &ContextTarget,
        id: Identifier,
        replacement: EntityState,
        snapshot: Option<&EntitySnapshot>,
        now: SystemInstant,
    ) -> Result<PlannedWrite<RequestedState>, Box<(Identifier, EntityError)>> {
        let Some(snapshot) = snapshot else {
            let write = self.chunk_insert(target, id, replacement, now)?;
            return built(write);
        };
        let entry_id = id.clone(); // clone: the entry is kept to rebuild the write if the store refuses it
        let states = UpdatedStates {
            previous: snapshot,
            current: replacement,
        };
        match self.chunk_update(target, id, states, now)? {
            Some(write) => built(write),
            // The replacement restated the entity this attempt read, so that
            // state is the replacement.
            None => Ok(PlannedWrite {
                entry: RequestedState::Built {
                    id: entry_id,
                    state: Arc::clone(&snapshot.state),
                },
                write: None,
            }),
        }
    }
}
