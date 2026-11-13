//! Batch upsert in update mode (clause 5.6.8.4): an Entity that exists is
//! updated by clause 5.6.2, one that does not is created.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        admitted_batch::{AdmittedBatchEntity, prepared_chunk},
        chunk::{ChunkWrite, UpdatedStates},
        chunk_read_modify_write::{ChunkRead, ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
        requested_state::stored_state,
        upsert::UpsertOutcome,
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    side_effect::SideEffectPlacement,
    write_admission::EntityWrite,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use std::sync::Arc;

/// What an update-mode upsert is built from on each attempt.
enum UpsertFragment {
    /// The request's prepared document, applied as an update to the entity
    /// an attempt reads.
    Requested(Box<PreparedEntity>),
    /// The entity an earlier attempt inserted for an entity it found absent.
    /// The insert lost to a concurrent create, so a later attempt updates the
    /// entity with it.
    Inserted {
        /// The entity the request is for.
        id: Identifier,
        /// The entity the insert stored.
        stored: Arc<EntityState>,
    },
}

/// Updating each entity of a chunk, or creating it when an attempt finds it
/// absent (clause 5.6.8.4, update mode).
struct UpdateRewrite<'a> {
    /// The service whose update or insert builds each write.
    service: &'a EntityService,
    /// The branch the chunk writes.
    target: &'a ContextTarget,
}

impl ChunkRewrite for UpdateRewrite<'_> {
    type Entry = UpsertFragment;

    const READ: ChunkRead = ChunkRead::Snapshots;

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
        now: SystemInstant,
    ) -> Result<PlannedWrite<UpsertFragment>, Box<(Identifier, EntityError)>> {
        let (target, service) = (self.target, self.service);
        match (entry, snapshot) {
            (UpsertFragment::Requested(prepared), Some(snapshot)) => {
                let write = service.upsert_update_write(target, prepared.id.clone(), &prepared.document, snapshot, now)?; // clone: the entry is kept to rebuild the write if the store refuses it
                Ok(PlannedWrite {
                    entry: UpsertFragment::Requested(prepared),
                    write,
                })
            }
            (UpsertFragment::Inserted { id, stored }, Some(snapshot)) => {
                let document = EntityDocument::from(&*stored);
                let write = service.upsert_update_write(target, id.clone(), &document, snapshot, now)?; // clone: the entry is kept to rebuild the write if the store refuses it
                Ok(PlannedWrite {
                    entry: UpsertFragment::Requested(Box::new(PreparedEntity { id, document })),
                    write,
                })
            }
            // Clause 5.6.8.4 routes an absent Entity to "the behaviour defined
            // by clause 5.6.1", which supports no NGSI-LD Null: clause 5.2.1
            // admits one only in a partial update or merge and answers
            // BadRequestData "in all other cases". The document is held to the
            // Null admission of a create, which the screening relaxed for the
            // update branch.
            (UpsertFragment::Requested(prepared), None) => {
                if let Err(refusal) = prepared.document.admit_null_placements(EntityWrite::Create.null_sentinel_admission()) {
                    return Err(Box::new((prepared.id, EntityError::from(refusal))));
                }
                let (id, state) = prepared.into_state()?;
                inserted(service.chunk_insert(target, id, state, now)?)
            }
            (UpsertFragment::Inserted { id, stored }, None) => inserted(service.chunk_insert(target, id, Arc::unwrap_or_clone(stored), now)?),
        }
    }
}

/// The planned insert of an update-mode upsert, keeping what a later attempt
/// rebuilds the request from if a concurrent create got there first.
///
/// A lost race reads the stored entity back as the written document it is:
/// the insert moved the request into the entity instead of keeping a copy of
/// it, and a request the create branch admitted carried no NGSI-LD Null, so
/// the document is the request. The system times the insert stamped come back
/// with it, and the update stamps every instance it writes against the entity
/// it found, so they leave no trace.
///
/// # Errors
///
/// Reports the invariant when the insert stores no entity.
fn inserted(write: ChunkWrite) -> Result<PlannedWrite<UpsertFragment>, Box<(Identifier, EntityError)>> {
    Ok(PlannedWrite {
        entry: UpsertFragment::Inserted {
            id: write.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            stored: stored_state(&write)?,
        },
        write: Some(write),
    })
}

impl EntityService {
    /// Update each entity of `chunk` at `now`, creating the absent ones. Per-entity
    /// failures are pushed onto `errors`; the outcome lists the created identifiers
    /// apart. Fails only on a store failure that concerns the whole chunk.
    pub(in crate::batch) async fn batch_upsert_update_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        errors: &mut Vec<(Identifier, EntityError)>,
        now: SystemInstant,
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
        // refuses the entity when its payload carries an NGSI-LD Null: that
        // branch is clause 5.6.1, which supports none.
        let prepared = prepared_chunk(chunk);
        let rewrite = UpdateRewrite {
            service: self,
            target: placement.target(),
        };
        let entries = prepared.into_iter().map(|prepared| UpsertFragment::Requested(Box::new(prepared))).collect();
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

    /// The write that updates `previous` with the request `fragment` (clause
    /// 5.6.8.4 update mode runs Update Attributes, the same whole-instance
    /// overwrite as a single update).
    ///
    /// Mirrors the single attribute update (clause 5.6.2): the per-attribute
    /// triggers, partitioned from the before/after diff, and no write at all
    /// when the fragment changes nothing.
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
        previous: &EntitySnapshot,
        now: SystemInstant,
    ) -> Result<Option<ChunkWrite>, Box<(Identifier, EntityError)>> {
        let mut updated = EntityState::clone(&previous.state);
        if let Err(error) = updated.update_attributes(fragment) {
            return Err(Box::new((id, EntityError::from(error))));
        }
        self.chunk_update(target, id, UpdatedStates { previous, current: updated }, now)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        admission_fixture::admitted_batch,
        batch::UpsertPolicy,
        compare_and_set_fixture::{Watchers, building_id, contended_service},
        entity_validation_reason::EntityValidationReason,
        error::EntityError,
        side_effect::SideEffectPlacement,
        write_kind::BatchUpsert,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::refusal::EntityRefusal;
    use serde_json::json;

    /// Clause 5.6.8.4 creates an absent Entity by clause 5.6.1, which admits
    /// no NGSI-LD Null, although the update branch's screening admits the Null
    /// as a first-level member.
    #[tokio::test]
    async fn an_update_mode_upsert_creates_no_entity_typed_as_the_null() {
        let contended = contended_service(Watchers::None).await;
        let id = building_id("null-typed");
        let entity = serde_json::from_value(json!({"id": id.as_str(), "type": ["Building", "urn:ngsi-ld:null"]})).expect("the compact model reads it");
        let admitted = admitted_batch(&contended.service, BatchUpsert(UpsertPolicy::Update), vec![entity], &ContextSpec::core_only()).await;
        let (result, created) = contended
            .service
            .batch_upsert(SideEffectPlacement::Inline(&ContextTarget::Live), admitted)
            .await
            .expect("the batch completes");
        assert!(created.is_empty() && result.success.is_empty(), "{created:?} {:?}", result.success);
        assert!(
            matches!(
                result.errors.as_slice(),
                [(refused, EntityError::Validation(EntityValidationReason::Refused(refusal)))] if *refused == id && matches!(**refusal, EntityRefusal::TypeNull)
            ),
            "{:?}",
            result.errors
        );
        assert!(contended.store.stored(&id).is_none(), "nothing is stored");
    }
}
