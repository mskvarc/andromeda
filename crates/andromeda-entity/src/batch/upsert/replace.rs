//! Batch upsert in replace mode (clause 5.6.8.4): an Entity that exists is
//! replaced, one that does not is created.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        admitted_batch::{AdmittedBatchEntity, prepared_chunk},
        chunk::{ChunkWrite, UpdatedStates},
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
        upsert::{UpsertOutcome, stored_state},
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use std::sync::Arc;

/// What a replacement is built from on each attempt.
enum Replacement {
    /// The request's prepared document, which no attempt has spent yet.
    Requested(Box<PreparedEntity>),
    /// The replacement an earlier attempt stored, kept so a later attempt does
    /// not convert the request again.
    ///
    /// Only this entry holds it once the refused write is dropped, so a later
    /// attempt takes it back without copying it; its system times are stamped
    /// again against the state that attempt reads.
    Built {
        /// The entity the replacement is for.
        id: Identifier,
        /// The replacement as the earlier attempt stored it.
        state: Arc<EntityState>,
    },
}

/// Replacing each entity of a chunk, or creating it when an attempt finds it
/// absent (clause 5.6.8.4, replace mode).
struct ReplaceRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
}

impl ChunkRewrite for ReplaceRewrite<'_> {
    type Entry = Replacement;
    type Reads = ();

    fn id(entry: &Replacement) -> &Identifier {
        match entry {
            Replacement::Requested(prepared) => &prepared.id,
            Replacement::Built { id, .. } => id,
        }
    }

    fn build(
        &self,
        entry: Replacement,
        snapshot: Option<&EntitySnapshot>,
        _reads: &(),
        now: SystemInstant,
    ) -> Result<PlannedWrite<Replacement>, Box<(Identifier, EntityError)>> {
        let (id, replacement) = match entry {
            Replacement::Requested(prepared) => prepared.into_state()?,
            // clone: reference-count bumps only when the earlier attempt's write still shares the state
            Replacement::Built { id, state } => (id, Arc::unwrap_or_clone(state)),
        };
        self.service.replacement_write(self.target, id, replacement, snapshot, now)
    }
}

impl EntityService {
    pub(in crate::batch) async fn batch_upsert_replace_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        errors: &mut Vec<(Identifier, EntityError)>,
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
        let entries = prepared.into_iter().map(|prepared| Replacement::Requested(Box::new(prepared))).collect();
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, entries).await?;
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
    ) -> Result<PlannedWrite<Replacement>, Box<(Identifier, EntityError)>> {
        let Some(snapshot) = snapshot else {
            let write = self.chunk_insert(target, id, replacement, now)?;
            return built(write);
        };
        let entry_id = id.clone(); // clone: the entry is kept to rebuild the write if the store refuses it
        let states = UpdatedStates {
            previous: &snapshot.state,
            current: replacement,
        };
        match self.chunk_update(target, id, states, now)? {
            Some(write) => built(write),
            // The replacement restated the entity this attempt read, so that
            // state is the replacement.
            None => Ok(PlannedWrite {
                entry: Replacement::Built {
                    id: entry_id,
                    state: Arc::clone(&snapshot.state), // clone: Arc ref-count bump
                },
                write: None,
                alongside: None,
            }),
        }
    }
}

/// The planned write storing `write`, kept so a refused attempt rebuilds from
/// the replacement it stored.
fn built(write: ChunkWrite) -> Result<PlannedWrite<Replacement>, Box<(Identifier, EntityError)>> {
    Ok(PlannedWrite {
        entry: Replacement::Built {
            id: write.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            state: stored_state(&write)?,
        },
        write: Some(write),
        alongside: None,
    })
}
