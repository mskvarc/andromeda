//! Batch entity creation (clause 5.6.7).

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
use andromeda_ngsi_ld_model::entity::Entity;
use std::collections::HashSet;

impl EntityService {
    /// Reject the second and later appearances of a repeated identifier.
    ///
    /// A batch that names one entity twice is asking to create it twice, and
    /// the second is a conflict exactly as it would be in a separate request
    /// (NGSI-LD 6.4.3.1).
    pub(super) fn dedupe_create_entities(entities: &[Entity]) -> (Vec<&Entity>, Vec<(Uri, AndromedaError)>) {
        let mut deduped = Vec::with_capacity(entities.len());
        let mut errors = Vec::new();
        let mut seen = HashSet::with_capacity(entities.len());

        for entity in entities {
            let id = &entity.id;
            if seen.insert(id) {
                deduped.push(entity);
            } else {
                errors.push(((*id).clone(), AndromedaError::entity_already_exists(id.as_str()))); // clone: error result owns the entity ID
            }
        }

        (deduped, errors)
    }

    pub(super) async fn batch_create_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entity_refs(entities, context).await;

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let lock_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: guards outlive the borrowed preparation view
        let _id_guards = self.acquire_id_locks(&lock_ids).await;

        let mut seen: HashSet<Uri> = HashSet::with_capacity(prepared.len());
        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            if seen.insert(entity.id.clone()) {
                writes.push(ChunkWrite::insert(entity, Precondition::MustNotExist));
            } else {
                errors.push((entity.id.clone(), AndromedaError::entity_already_exists(entity.id.as_str()))); // clone: error result owns the entity ID
            }
        }

        let outcome = self.commit_chunk(writes, mode).await?.settled();
        errors.extend(outcome.errors);

        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
