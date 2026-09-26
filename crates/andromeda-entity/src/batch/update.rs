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
    entity_validation_reason::EntityValidationReason,
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

/// Whether a `noOverwrite` append of `attrs` onto `entity` can add nothing: the
/// fragment names at least one attribute and every named attribute is already
/// present. Clause 5.6.9.5 counts an Entity in S only when it was "successfully
/// updated (Attributes appended)", so an append that adds no attribute leaves
/// the Entity out of S (clause 5.6.9.4). A fragment that names a not-yet-present
/// attribute appends it and the Entity is a success even if others are skipped,
/// matching the single Append Attributes operation (clause 5.6.3), which reports
/// the skipped ones in a `notUpdated` list rather than failing the Entity.
fn appends_nothing(entity: &Value, attrs: &Value) -> bool {
    let entity_obj = match entity {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };
    let Some((entity_obj, attrs_obj)) = entity_obj.zip(attrs.as_object()) else {
        return false;
    };
    let mut named = attrs_obj.keys().filter(|key| !key.starts_with('@')).peekable();
    named.peek().is_some() && named.all(|key| entity_obj.contains_key(key))
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
                // Clause 5.6.9.4: the local phase is an Append Attributes (clause
                // 5.6.3) with overwrite disabled. Clause 5.6.9.5 puts an Entity in
                // S when it was "successfully updated (Attributes appended)", so a
                // fragment that appends at least one attribute is a success even if
                // others are skipped for noOverwrite; only a fragment that appends
                // nothing (every named attribute already present) leaves the Entity
                // out of S and is reported as an error, making the batch a 207.
                let appended_nothing = appends_nothing(&existing, attrs);
                if let Err(error) = append_entity_attrs(&mut existing, attrs, OverwritePolicy::Preserve) {
                    errors.push((id, error.into()));
                    continue;
                }
                if appended_nothing {
                    errors.push((
                        id.clone(), // clone: the write below still owns the identifier
                        EntityError::Validation(EntityValidationReason::NoOverwriteConflict),
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
            let groups = match attribute_change_groups(previous, &existing) {
                Ok(groups) => groups,
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
                    observed: Arc::new(prepared_entity.expanded),
                },
                &groups,
                Precondition::MustExist,
                now,
            ) {
                Ok(write) => writes.push(write),
                Err(failure) => errors.push(*failure),
            }
        }

        let outcome = self.commit_chunk(target, writes, mode).await?.settled();
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::batch::update::appends_nothing;
    use serde_json::json;

    /// Clause 5.6.9.5: an Entity is a success (S) only when at least one Attribute
    /// was appended. A fragment naming a not-yet-present Attribute appends it, even
    /// beside already-present ones; a fragment naming only present Attributes appends
    /// nothing.
    #[test]
    fn a_no_overwrite_fragment_appends_nothing_only_when_every_named_attribute_is_present() {
        let stored = json!({"@id": "urn:ngsi-ld:Vehicle:1", "brandName": {"type": "Property"}, "isParked": {"type": "Relationship"}});

        // Every named attribute already present -> appends nothing (207 path).
        assert!(appends_nothing(&stored, &json!({"isParked": {"type": "Relationship"}})));
        assert!(appends_nothing(
            &stored,
            &json!({"@context": "x", "brandName": {"type": "Property"}, "isParked": {"type": "Relationship"}})
        ));

        // At least one named attribute is new -> appends something (S / 204 path).
        assert!(!appends_nothing(&stored, &json!({"speed": {"type": "Property"}})));
        assert!(!appends_nothing(
            &stored,
            &json!({"isParked": {"type": "Relationship"}, "speed": {"type": "Property"}})
        ));

        // A fragment naming no attribute (only JSON-LD keywords) appends nothing new
        // but is not a noOverwrite conflict either.
        assert!(!appends_nothing(&stored, &json!({"@context": "x"})));

        // The stored payload may arrive array-wrapped.
        let array_wrapped = json!([{"brandName": {"type": "Property"}}]);
        assert!(appends_nothing(&array_wrapped, &json!({"brandName": {"type": "Property"}})));
        assert!(!appends_nothing(&array_wrapped, &json!({"speed": {"type": "Property"}})));
    }
}
