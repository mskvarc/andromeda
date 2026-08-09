//! Batch attribute update (clause 5.6.9).
//!
//! A read-modify-write over the whole chunk: the stored payloads are read
//! once, each request fragment is applied in memory, and the results go back
//! as one statement. The per-id guards are held across the whole window so two
//! concurrent batches touching one entity cannot lose each other's writes.

use crate::{
    error::AndromedaError,
    service::entity::{
        EntityService,
        batch::{BatchResult, chunk::ChunkWrite},
        expanded_member::unwrap_expanded_ref,
        precondition::Precondition,
        side_effect::BatchSideEffectMode,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::mutation::{
        entity::{append_entity_attrs, update_entity_attrs},
        selection::OverwritePolicy,
    },
};
use serde_json::Value;
use std::sync::Arc;

/// Check if any non-@-prefixed attribute keys in `attrs` already exist in `entity`.
fn has_overlapping_attrs(entity: &Value, attrs: &Value) -> bool {
    let entity_obj = match entity {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        _other => None,
    };
    let attrs_obj = attrs.as_object();

    if let (Some(entity_obj), Some(attrs_obj)) = (entity_obj, attrs_obj) {
        attrs_obj.keys().any(|key| !key.starts_with('@') && entity_obj.contains_key(key))
    } else {
        false
    }
}

impl EntityService {
    pub(super) async fn batch_update_chunk(
        &self,
        entities: &[Entity],
        context: &ContextSpec,
        overwrite: OverwritePolicy,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent updates for the same id can't lose each other's writes.
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

            let attrs = unwrap_expanded_ref(&prepared_entity.expanded);
            if overwrite == OverwritePolicy::Preserve {
                let skipped = has_overlapping_attrs(&existing, attrs);
                if let Err(error) = append_entity_attrs(&mut existing, attrs, OverwritePolicy::Preserve) {
                    errors.push((id, error.into()));
                    continue;
                }
                if skipped {
                    errors.push((
                        id.clone(), // clone: the write below still owns the identifier
                        AndromedaError::entity_validation("Some attributes already exist and were not updated due to noOverwrite"),
                    ));
                }
            } else {
                if let Err(error) = update_entity_attrs(&mut existing, attrs) {
                    errors.push((id, error.into()));
                    continue;
                }
                if let Err(error) = append_entity_attrs(&mut existing, attrs, OverwritePolicy::Overwrite) {
                    errors.push((id, error.into()));
                    continue;
                }
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
