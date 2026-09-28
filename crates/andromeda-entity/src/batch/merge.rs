//! Batch merge-patch.
//!
//! Like batch update, a read-modify-write over the whole chunk; unlike it, the
//! fragment is applied with clause 5.6.2 merge semantics rather than by
//! updating and appending in turn.
//!
//! A chunk carries at most one instance of any entity, because the coordinator
//! gives each instance of a repeated identifier a round of its own. That is
//! what makes the stored entity "the entity resulting from merging all of the
//! entity instances, in the correct order" and puts every instance into the
//! Temporal Evolution of Entities (clause 5.5.11.5).

use crate::{
    EntityService,
    batch::{BatchResult, admitted_batch::AdmittedBatchEntity, chunk::UpdatedPayloads},
    error::EntityError,
    notification_plan::modification,
    side_effect::SideEffectPlacement,
    write_admission::BatchWrite,
};
use andromeda_context_commit::precondition::Precondition;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::expanded::{
    mutation::{entity::merge_entity, selection::OverwritePolicy},
    written_observation::AddressedInstances,
};
use chrono::Utc;
use std::sync::Arc;

impl EntityService {
    pub(super) async fn batch_merge_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        context: &ContextSpec,
    ) -> Result<BatchResult, EntityError> {
        let target = placement.target();
        // Clause 5.6.20 is the batch form of Merge Entity, whose fragments carry
        // the NGSI-LD Null to arbitrary depth (clause 5.5.12).
        let (prepared, mut errors) = self.prepare_admitted_chunk(chunk, context, BatchWrite::Merge.null_sentinel_admission()).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent merges for the same id can't lose each other's writes.
        let _id_guards = self.acquire_id_locks(target, &prepared_ids).await;
        let existing_map = self.reader.get_batch(target, &prepared_ids).await?;
        let now = Utc::now();

        let mut writes = Vec::with_capacity(prepared.len());
        for prepared_entity in prepared {
            let id = prepared_entity.id;
            let Some(previous) = existing_map.get(&id) else {
                let error = EntityError::not_found(&id);
                errors.push((id, error));
                continue;
            };
            let mut existing = previous.clone(); // clone: the merge mutates its own copy of the stored payload

            if let Err(error) = merge_entity(&mut existing, &prepared_entity.expanded, None) {
                errors.push((id, error.into()));
                continue;
            }

            let change = match modification(previous, &existing) {
                Ok(change) => change,
                Err(error) => {
                    errors.push((id, error));
                    continue;
                }
            };

            match self.chunk_update(
                target,
                id,
                UpdatedPayloads {
                    previous,
                    merged: Arc::new(existing),
                    addressed: AddressedInstances::EntityFragment {
                        fragment: &prepared_entity.expanded,
                        overwrite: OverwritePolicy::Overwrite,
                    },
                },
                change,
                Precondition::MustExist,
                now,
            ) {
                Ok(write) => writes.push(write),
                Err(failure) => errors.push(*failure),
            }
        }

        let outcome = self.commit_chunk(placement, writes).await?.settled();
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
