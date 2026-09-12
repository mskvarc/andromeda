//! Batch attribute update (clause 5.6.9).
//!
//! A read-modify-write over the whole chunk: the stored payloads are read
//! once, each request fragment is applied in memory, and the results go back
//! as one statement. The per-id guards are held across the whole window so two
//! concurrent batches touching one entity cannot lose each other's writes.
//!
//! Reading the stored payloads once is only sound because a chunk never carries
//! two instances of the same entity: the coordinator gives each instance of a
//! repeated identifier a round of its own, so the second instance reads what
//! the first committed. Clause 5.5.11.3 requires exactly that, one separate
//! notification per individual update.

use crate::{
    EntityService,
    batch::{BatchResult, chunk::UpdatedPayloads},
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    notification_plan::attribute_change_groups,
    side_effect::BatchSideEffectMode,
    write_admission::BatchWrite,
};
use andromeda_context_commit::precondition::Precondition;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::mutation::{
        entity::{append_entity_attrs, update_entity_attrs},
        selection::OverwritePolicy,
    },
};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

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
        target: &ContextTarget,
        entities: &[&Entity],
        context: &ContextSpec,
        overwrite: OverwritePolicy,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, EntityError> {
        // Clause 5.5.4 excepts only the fragments "used in partial update and
        // merge operations" from its NGSI-LD Null prohibitions, and clause 5.2.1
        // makes every other placement of the sentinel `BadRequestData`. A batch
        // update is neither: clause 5.6.9.4's local phase names "the behaviour
        // defined by clause 5.6.3" -- Append Attributes, which carries no
        // NGSI-LD Null support of its own -- and clause 5.6.9 does not carry the
        // "NGSI-LD Nulls should be supported by this operation" sentence that
        // clauses 5.6.2, 5.6.4 and 5.6.17 do. The sentinel therefore has no
        // deletion semantics to be given here, and storing it would put
        // "urn:ngsi-ld:null" in a value the broker later serves back.
        //
        // What the code below runs is not clause 5.6.3 for the overwriting
        // default of clause 5.6.9.3: an overwriting update applies the clause
        // 5.6.2 patch first and appends what it left, and only `noOverwrite`
        // appends alone. That deviation is recorded in
        // `docs/spec-conformance-sweep.md`; it does not move the admission,
        // because clause 5.6.2's NGSI-LD Null support is a statement about the
        // Update Attributes operation and its Entity Fragment, not about a
        // whole Entity submitted to a batch.
        let (prepared, mut errors) = self.prepare_entity_refs(entities, context, BatchWrite::Update.null_sentinel_admission()).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent updates for the same id can't lose each other's writes.
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
            let groups = attribute_change_groups(previous, &existing);

            writes.push(self.chunk_update(
                target,
                id,
                UpdatedPayloads {
                    previous,
                    merged: Arc::new(existing),
                    observed: Arc::new(prepared_entity.expanded),
                },
                &groups,
                Precondition::MustExist,
                now,
            ));
        }

        let outcome = self.commit_chunk(target, writes, mode).await?.settled();
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
