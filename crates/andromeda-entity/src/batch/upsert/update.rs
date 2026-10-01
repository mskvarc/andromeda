//! Batch upsert in update mode (clause 5.6.8.4): an Entity that exists is
//! updated by clause 5.6.2, one that does not is created.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        UpsertPolicy,
        admitted_batch::AdmittedBatchEntity,
        chunk::{ChunkWrite, UpdatedPayloads},
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
        upsert::{UpsertOutcome, stored_payload},
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    notification_plan::modification,
    side_effect::SideEffectPlacement,
    write_admission::BatchWrite,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{
    mutation::{
        deletion_sentinel::{DroppedSentinels, drop_deletion_sentinels, restore_deletion_sentinels},
        entity::update_entity_attrs,
        selection::OverwritePolicy,
    },
    written_observation::AddressedInstances,
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::sync::Arc;

/// What an update-mode upsert is built from on each attempt.
enum UpsertFragment {
    /// The request's prepared payload, which no attempt has spent yet.
    Requested(Box<PreparedEntity>),
    /// The payload an earlier attempt inserted for an entity it found absent,
    /// and the deletions it dropped from the request to do so. The insert lost
    /// to a concurrent create, so a later attempt updates the entity with the
    /// two put back together.
    Inserted {
        /// The entity the request is for.
        id: Identifier,
        /// The payload the insert stored.
        stored: Arc<Value>,
        /// The members of the request that asked for a deletion.
        deletions: Map<String, Value>,
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
        now: DateTime<Utc>,
    ) -> Result<PlannedWrite<UpsertFragment>, Box<(Identifier, EntityError)>> {
        let (target, service) = (self.target, self.service);
        match (entry, snapshot) {
            (UpsertFragment::Requested(prepared), Some(snapshot)) => {
                let write = service.upsert_update_write(target, prepared.id.clone(), &prepared.expanded, &snapshot.payload, now)?; // clone: the entry is kept to rebuild the write if the store refuses it
                Ok(PlannedWrite {
                    entry: UpsertFragment::Requested(prepared),
                    write,
                    alongside: None,
                })
            }
            (UpsertFragment::Inserted { id, stored, deletions }, Some(snapshot)) => {
                // The stored payload carries the system times the insert
                // stamped; the update restamps every instance it writes against
                // the entity it found, so they leave no trace.
                let mut fragment = Value::clone(&stored); // clone: the update needs the request back, and the stored payload is shared with the refused insert
                restore_deletion_sentinels(&mut fragment, &deletions);
                let write = service.upsert_update_write(target, id.clone(), &fragment, &snapshot.payload, now)?; // clone: the entry is kept to rebuild the write if the store refuses it
                Ok(PlannedWrite {
                    entry: UpsertFragment::Inserted { id, stored, deletions },
                    write,
                    alongside: None,
                })
            }
            // Clause 5.6.8.4 routes an absent Entity to "the behaviour defined
            // by clause 5.6.1", where the deletions the update branch admits
            // have nothing to delete: they are dropped, and only a payload that
            // carried one is re-derived, so an ordinary create keeps the
            // columns the preparation pass computed.
            (UpsertFragment::Requested(prepared), None) => {
                let mut prepared = *prepared;
                let (write, deletions) = match drop_deletion_sentinels(&mut prepared.expanded) {
                    DroppedSentinels::Unchanged => (service.chunk_insert(target, prepared, now)?, Map::new()),
                    DroppedSentinels::Dropped(deletions) => (service.chunk_insert_payload(target, prepared.id, Arc::new(prepared.expanded), now)?, deletions),
                };
                inserted(write, deletions)
            }
            (UpsertFragment::Inserted { id, stored, deletions }, None) => inserted(service.chunk_insert_payload(target, id, stored, now)?, deletions),
        }
    }
}

/// The planned insert of an update-mode upsert, keeping what a later attempt
/// rebuilds the request from if a concurrent create got there first.
///
/// # Errors
///
/// Reports the invariant when the insert stores no payload.
fn inserted(write: ChunkWrite, deletions: Map<String, Value>) -> Result<PlannedWrite<UpsertFragment>, Box<(Identifier, EntityError)>> {
    Ok(PlannedWrite {
        entry: UpsertFragment::Inserted {
            id: write.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            stored: stored_payload(&write)?,
            deletions,
        },
        write,
        alongside: None,
    })
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
    /// failure is the rare outcome, when the fragment cannot be applied or
    /// either state's members cannot be read.
    fn upsert_update_write(
        &self,
        target: &ContextTarget,
        id: Identifier,
        fragment: &Value,
        previous: &Arc<Value>,
        now: DateTime<Utc>,
    ) -> Result<ChunkWrite, Box<(Identifier, EntityError)>> {
        let mut updated = Value::clone(previous); // clone: the update mutates its own copy of the stored payload
        if let Err(error) = update_entity_attrs(&mut updated, unwrap_expanded_ref(fragment)) {
            return Err(Box::new((id, error.into())));
        }
        let change = match modification(previous, &updated) {
            Ok(change) => change,
            Err(error) => return Err(Box::new((id, error))),
        };
        self.chunk_update(
            target,
            id,
            UpdatedPayloads {
                previous,
                merged: Arc::new(updated),
                addressed: AddressedInstances::EntityFragment {
                    fragment,
                    overwrite: OverwritePolicy::Overwrite,
                },
            },
            change,
            now,
        )
    }
}
