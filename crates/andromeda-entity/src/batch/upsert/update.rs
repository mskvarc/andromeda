//! Batch upsert in update mode (clause 5.6.8.4): an Entity that exists is
//! updated by clause 5.6.2, one that does not is created.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        UpsertPolicy,
        admitted_batch::AdmittedBatchEntity,
        chunk::{ChunkWrite, UpdatedStates},
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
        upsert::{UpsertOutcome, stored_state},
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    side_effect::SideEffectPlacement,
    write_admission::BatchWrite,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{
    document::EntityDocument,
    entity_state::EntityState,
    history::written_observation::AddressedInstances,
    operation::{deletion_drop::DroppedDeletions, selection::OverwritePolicy},
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::sync::Arc;

/// What an update-mode upsert is built from on each attempt.
enum UpsertFragment {
    /// The request's prepared document, applied as an update to the entity
    /// an attempt reads.
    Requested(Box<PreparedEntity>),
    /// The entity an earlier attempt inserted for an entity it found absent,
    /// and the deletions it dropped from the request to do so. The insert lost
    /// to a concurrent create, so a later attempt updates the entity with the
    /// two put back together.
    Inserted {
        /// The entity the request is for.
        id: Identifier,
        /// The entity the insert stored.
        stored: Arc<EntityState>,
        /// The members of the request that asked for a deletion, boxed
        /// because they are rarely more than a marker and the entry is moved
        /// through every attempt.
        deletions: Box<DroppedDeletions>,
    },
}

/// Updating each entity of a chunk, or creating it when an attempt finds it
/// absent (clause 5.6.8.4, update mode).
struct UpdateRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
}

impl ChunkRewrite for UpdateRewrite<'_> {
    type Entry = UpsertFragment;
    type Reads = ();

    fn id(entry: &UpsertFragment) -> &Identifier {
        match entry {
            UpsertFragment::Requested(prepared) => &prepared.id,
            UpsertFragment::Inserted { id, .. } => id,
        }
    }

    fn build(
        &self,
        entry: UpsertFragment,
        snapshot: Option<&EntitySnapshot>,
        _reads: &(),
        now: SystemInstant,
    ) -> Result<PlannedWrite<UpsertFragment>, Box<(Identifier, EntityError)>> {
        let (target, service) = (self.target, self.service);
        match (entry, snapshot) {
            (UpsertFragment::Requested(prepared), Some(snapshot)) => {
                let write = service.upsert_update_write(target, prepared.id.clone(), &prepared.document, &snapshot.state, now)?; // clone: the entry is kept to rebuild the write if the store refuses it
                Ok(PlannedWrite {
                    entry: UpsertFragment::Requested(prepared),
                    write,
                    alongside: None,
                })
            }
            (UpsertFragment::Inserted { id, stored, deletions }, Some(snapshot)) => {
                let document = requested_again(&stored, *deletions);
                let write = service.upsert_update_write(target, id.clone(), &document, &snapshot.state, now)?; // clone: the entry is kept to rebuild the write if the store refuses it
                Ok(PlannedWrite {
                    entry: UpsertFragment::Requested(Box::new(PreparedEntity { id, document })),
                    write,
                    alongside: None,
                })
            }
            // Clause 5.6.8.4 routes an absent Entity to "the behaviour defined
            // by clause 5.6.1", where the deletions the update branch admits
            // have nothing to delete: they are dropped, and the rest of the
            // request becomes the entity.
            (UpsertFragment::Requested(prepared), None) => {
                let mut prepared = *prepared;
                let deletions = Box::new(prepared.document.drop_deletions());
                let (id, state) = prepared.into_state()?;
                inserted(service.chunk_insert(target, id, state, now)?, deletions)
            }
            // clone: reference-count bumps only when the earlier attempt's write still shares the state
            (UpsertFragment::Inserted { id, stored, deletions }, None) => {
                inserted(service.chunk_insert(target, id, Arc::unwrap_or_clone(stored), now)?, deletions)
            }
        }
    }
}

/// The planned insert of an update-mode upsert, keeping what a later attempt
/// rebuilds the request from if a concurrent create got there first.
///
/// # Errors
///
/// Reports the invariant when the insert stores no entity.
fn inserted(write: ChunkWrite, deletions: Box<DroppedDeletions>) -> Result<PlannedWrite<UpsertFragment>, Box<(Identifier, EntityError)>> {
    Ok(PlannedWrite {
        entry: UpsertFragment::Inserted {
            id: write.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            stored: stored_state(&write)?,
            deletions,
        },
        write,
        alongside: None,
    })
}

/// The request an insert that lost to a concurrent create was built from: the
/// entity the insert stored, read as the written document it is, with the
/// deletions the insert dropped put back.
///
/// Only a lost race takes this path; the insert itself moved the request into
/// the entity instead of keeping a copy of it. The system times the insert
/// stamped come back with it, and the update stamps every instance it writes
/// against the entity it found, so they leave no trace.
fn requested_again(stored: &EntityState, deletions: DroppedDeletions) -> EntityDocument {
    let mut document = EntityDocument::from(stored);
    match deletions {
        DroppedDeletions::Dropped(members) => document.restore_deletions(members),
        DroppedDeletions::Unchanged => {}
    }
    document
}

impl EntityService {
    pub(in crate::batch) async fn batch_upsert_update_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        errors: &mut Vec<(Identifier, EntityError)>,
    ) -> Result<UpsertOutcome, EntityError> {
        // Clause 5.6.8.4: "If there were an existing Entity with the same Entity
        // ID, the behaviour defined by clause 5.6.2 shall be executed, but
        // limited to a local operation, if the requested update mode is
        // 'update'." Clause 5.6.2.4 is one of the three clauses carrying "NGSI-LD
        // Nulls should be supported by this operation", so this mode takes the
        // partial update fragment's admission and not the create branch's.
        //
        // Whether an Entity exists is not known while the payload is screened,
        // and the same payload creates it when it does not. The screening
        // therefore states the update branch's admission, and the create branch
        // drops the deletions rather than storing them: clause 5.5.8 deletes
        // "each member of the Fragment, whose value is an NGSI-LD Null,
        // contained by the target", and a target that does not exist contains
        // none of them.
        let (prepared, mut chunk_errors) = self
            .prepare_admitted_chunk(chunk, BatchWrite::Upsert(UpsertPolicy::Update).null_sentinel_admission())
            .await;
        errors.append(&mut chunk_errors);
        let rewrite = UpdateRewrite {
            service: self,
            target: placement.target(),
        };
        let entries = prepared.into_iter().map(|prepared| UpsertFragment::Requested(Box::new(prepared))).collect();
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

    /// The write that updates `previous` with the request `fragment` (clause
    /// 5.6.8.4 update mode runs Update Attributes, the same whole-instance
    /// overwrite as a single update).
    ///
    /// Mirrors the single attribute update (clause 5.6.2): the per-attribute
    /// triggers, partitioned from the before/after diff.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, when the fragment cannot be applied.
    fn upsert_update_write(
        &self,
        target: &ContextTarget,
        id: Identifier,
        fragment: &EntityDocument,
        previous: &Arc<EntityState>,
        now: SystemInstant,
    ) -> Result<ChunkWrite, Box<(Identifier, EntityError)>> {
        // clone: reference-count bumps; the update shares every attribute it does not touch with the stored state
        let mut updated = EntityState::clone(previous);
        if let Err(error) = updated.update_attributes(fragment) {
            return Err(Box::new((id, EntityError::from(error))));
        }
        self.chunk_update(
            target,
            id,
            UpdatedStates {
                previous,
                current: updated,
                addressed: AddressedInstances::EntityFragment {
                    fragment,
                    overwrite: OverwritePolicy::Overwrite,
                },
            },
            now,
        )
    }
}
