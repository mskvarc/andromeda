//! Batch merge-patch.
//!
//! Like batch update, a read-modify-write over the whole chunk; unlike it, the
//! fragment is applied with clause 5.6.2 merge semantics rather than by
//! updating and appending in turn.

use crate::{EntityService, batch::BatchResult, error::EntityError, notification_plan::NotificationGroup, side_effect::BatchSideEffectMode};
use andromeda_context_commit::precondition::Precondition;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{entity_member::extract_attribute_names, mutation::entity::merge_entity},
    notification::NotificationTrigger,
};
use std::{collections::HashSet, sync::Arc};

impl EntityService {
    pub(super) async fn batch_merge_chunk(&self, entities: &[Entity], context: &ContextSpec, mode: BatchSideEffectMode) -> Result<BatchResult, EntityError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent merges for the same id can't lose each other's writes.
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
            let mut existing = previous.clone(); // clone: the merge mutates its own copy of the stored payload

            if let Err(error) = merge_entity(&mut existing, &prepared_entity.expanded) {
                errors.push((id, error.into()));
                continue;
            }

            // Batch merge mirrors the single merge (clause 5.6.17): one
            // attributeDeleted per removed attribute, then a single entityUpdated
            // over the patched names.
            let attrs_after: HashSet<AttributeName> = extract_attribute_names(&existing).into_iter().collect();
            let mut groups: Vec<NotificationGroup> = attrs_before
                .difference(&attrs_after)
                .map(|deleted| NotificationGroup {
                    trigger: NotificationTrigger::AttributeDeleted,
                    attributes: vec![deleted.clone()], // clone: the group owns the removed name
                })
                .collect();
            groups.push(NotificationGroup {
                trigger: NotificationTrigger::EntityUpdated,
                attributes: prepared_entity.changed_attributes,
            });
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
