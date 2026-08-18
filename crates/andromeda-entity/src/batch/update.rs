//! Batch attribute update (clause 5.6.9).
//!
//! A read-modify-write over the whole chunk: the stored payloads are read
//! once, each request fragment is applied in memory, and the results go back
//! as one statement. The per-id guards are held across the whole window so two
//! concurrent batches touching one entity cannot lose each other's writes.

use crate::{
    EntityService,
    batch::BatchResult,
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    notification_plan::attribute_change_groups,
    side_effect::BatchSideEffectMode,
};
use andromeda_context_commit::precondition::Precondition;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{
        entity_member::extract_attribute_names,
        mutation::{
            entity::{append_entity_attrs, update_entity_attrs},
            selection::OverwritePolicy,
        },
    },
};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

/// Check if any non-@-prefixed attribute keys in `attrs` already exist in `entity`.
fn has_overlapping_attrs(entity: &Value, attrs: &Value) -> bool {
    let entity_obj = match entity {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
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
    ) -> Result<BatchResult, EntityError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent updates for the same id can't lose each other's writes.
        let _id_guards = self.acquire_id_locks(&prepared_ids).await;
        let existing_map = self.reader.get_batch(&prepared_ids).await?;

        let mut writes = Vec::with_capacity(prepared.len());
        for prepared_entity in prepared {
            let id = prepared_entity.id;
            let Some(previous) = existing_map.get(&id) else {
                let error = EntityError::not_found(&id);
                errors.push((id, error));
                continue;
            };
            let attrs_before: HashSet<AttributeName> = extract_attribute_names(previous).into_iter().collect();
            let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload

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
                        EntityError::validation("Some attributes already exist and were not updated due to noOverwrite"),
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

            // Batch update mirrors the single attribute update (clause 5.6.9):
            // the per-attribute triggers, partitioned from the before/after diff.
            let attrs_after: HashSet<AttributeName> = extract_attribute_names(&existing).into_iter().collect();
            let groups = attribute_change_groups(&attrs_before, &attrs_after, &prepared_entity.changed_attributes);
            let previous_snapshot = self.change_sink.as_ref().map(|_| previous.clone()); // clone: showChanges before-image, only when a subscriber needs it

            writes.push(self.chunk_update(
                id,
                previous,
                Arc::new(existing),
                Arc::new(prepared_entity.expanded),
                groups,
                previous_snapshot.as_ref(),
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
