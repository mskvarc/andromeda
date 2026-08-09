//! Batch merge-patch.
//!
//! Like batch update, a read-modify-write over the whole chunk; unlike it, the
//! fragment is applied with clause 5.6.2 merge semantics rather than by
//! updating and appending in turn.

use crate::{
    error::AndromedaError,
    service::entity::{
        EntityService,
        batch::{BatchResult, chunk::ChunkWrite},
        precondition::Precondition,
        side_effect::BatchSideEffectMode,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{entity::Entity, expanded::mutation::entity::merge_entity};
use std::sync::Arc;

impl EntityService {
    pub(super) async fn batch_merge_chunk(&self, entities: &[Entity], context: &ContextSpec, mode: BatchSideEffectMode) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent merges for the same id can't lose each other's writes.
        let _id_guards = self.acquire_id_locks(&prepared_ids).await;
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut writes = Vec::with_capacity(prepared.len());
        for prepared_entity in prepared {
            let id = prepared_entity.id;
            let Some(mut existing) = existing_map.get(&id).cloned() else {
                // clone: the merge mutates its own copy of the stored payload
                let error = AndromedaError::entity_not_found(id.as_str());
                errors.push((id, error));
                continue;
            };

            if let Err(error) = merge_entity(&mut existing, &prepared_entity.expanded) {
                errors.push((id, error.into()));
                continue;
            }

            writes.push(ChunkWrite::update(
                id,
                Arc::new(existing),
                Arc::new(prepared_entity.expanded),
                prepared_entity.changed_attributes,
                Precondition::MustExist,
            ));
        }

        let outcome = self.commit_chunk(writes, mode).await?.settled();
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
