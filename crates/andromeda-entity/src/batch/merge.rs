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
    batch::{
        BatchResult,
        admitted_batch::AdmittedBatchEntity,
        chunk::UpdatedPayloads,
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    notification_plan::modification,
    side_effect::SideEffectPlacement,
    write_admission::BatchWrite,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{
    mutation::{entity::merge_entity, selection::OverwritePolicy},
    written_observation::AddressedInstances,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// Merging each entity of a chunk into the entity as an attempt read it
/// (clause 5.6.20.4 runs the merge of clause 5.6.17 per entity).
struct MergeRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
}

impl ChunkRewrite for MergeRewrite<'_> {
    type Entry = PreparedEntity;
    type Reads = ();

    fn id(entry: &PreparedEntity) -> &Identifier {
        &entry.id
    }

    fn build(
        &self,
        entry: PreparedEntity,
        snapshot: Option<&EntitySnapshot>,
        _reads: &(),
        now: DateTime<Utc>,
    ) -> Result<PlannedWrite<PreparedEntity>, Box<(Identifier, EntityError)>> {
        let Some(snapshot) = snapshot else {
            let error = EntityError::not_found(&entry.id);
            return Err(Box::new((entry.id, error)));
        };
        let previous = &snapshot.payload;
        let mut merged = Value::clone(previous); // clone: the merge mutates its own copy of the stored payload
        if let Err(error) = merge_entity(&mut merged, &entry.expanded, None) {
            return Err(Box::new((entry.id, error.into())));
        }
        let change = match modification(previous, &merged) {
            Ok(change) => change,
            Err(error) => return Err(Box::new((entry.id, error))),
        };
        let write = self.service.chunk_update(
            self.target,
            entry.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            UpdatedPayloads {
                previous,
                merged: Arc::new(merged),
                addressed: AddressedInstances::EntityFragment {
                    fragment: &entry.expanded,
                    overwrite: OverwritePolicy::Overwrite,
                },
            },
            change,
            now,
        )?;
        Ok(PlannedWrite { entry, write, alongside: None })
    }
}

impl EntityService {
    pub(super) async fn batch_merge_chunk(&self, placement: SideEffectPlacement<'_>, chunk: Vec<AdmittedBatchEntity>) -> Result<BatchResult, EntityError> {
        // Clause 5.6.20 is the batch form of Merge Entity, whose fragments carry
        // the NGSI-LD Null to arbitrary depth (clause 5.5.12).
        let (prepared, mut errors) = self.prepare_admitted_chunk(chunk, BatchWrite::Merge.null_sentinel_admission()).await;
        let rewrite = MergeRewrite {
            service: self,
            target: placement.target(),
        };
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, prepared).await?;
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
