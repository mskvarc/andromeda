//! Replace Entity (clause 5.6.18).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    expanded_member::written_entity_types,
    notification_plan::modification,
    write_kind::ReplaceEntity,
};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord, history_record::mutation_history};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{
    attribute_system_time::stamp_attribute_instants,
    entity_member::{ExpandedMemberError, extract_id},
    mutation::{error::ExpandedEntityError, selection::OverwritePolicy},
    node::expanded_object_mut,
    written_observation::AddressedInstances,
};
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Replace an entire entity with its admitted payload (NGSI-LD 5.6.18).
    ///
    /// The replacement drops every attribute it omits, so each dropped instance
    /// needs a tombstone, and that diff is only meaningful against the payload
    /// the entity actually had when the write landed. The write therefore
    /// states the `modifiedAt` its diff assumed; a writer that got in first
    /// makes the store refuse it, and the entity is read again.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::NotFound`] when no entity carries `id`,
    /// [`EntityError::Validation`] when the payload's members cannot be read or
    /// it names an entity other than `id`,
    /// [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn replace_entity(&self, target: &ContextTarget, id: &Identifier, admitted: AdmittedEntity<ReplaceEntity>) -> Result<(), EntityError> {
        let timer = entity::start(EntityOperation::Replace);
        let result: Result<(), EntityError> = async {
            let entity_uri = id.clone(); // clone: mutation events and computed-write closures own the identifier
            if self.reader.get_snapshot(target, &entity_uri).await?.is_none() {
                return Err(EntityError::not_found(&entity_uri));
            }
            let expanded = identified_as(admitted.into_expanded(), &entity_uri)?;

            let entity_id = Arc::new(entity_uri.clone()); // clone: repeated computed-write attempts share the validated identifier.
            let now = Utc::now();

            // The types the notification carries are the stored ones, read off the
            // payload the replacement actually superseded rather than off an earlier
            // attempt's.
            let events = self
                .commit_computed_write(target, &entity_uri, |previous| {
                    // Clause 4.8: an attribute the replacement keeps (same datasetId)
                    // keeps its createdAt, matching the Entity-level createdAt column
                    // that a replace does not rewrite; every instance takes `now` for
                    // modifiedAt. Stamped per attempt against that attempt's stored
                    // payload, so createdAt survives the replace.
                    let mut replaced = expanded.clone(); // clone: each attempt stamps its own copy against the payload it superseded
                    stamp_attribute_instants(&mut replaced, previous, now).map_err(ExpandedEntityError::from)?;
                    let payload = Arc::new(replaced);
                    let types = written_entity_types(&payload)?;
                    let addressed = AddressedInstances::EntityFragment {
                        fragment: &payload,
                        overwrite: OverwritePolicy::Overwrite,
                    };
                    let history = mutation_history(addressed, previous, &payload)?;
                    let change = modification(previous, &payload)?;
                    let plan = self.notifier.plan(target, &entity_uri, &types, change, Some(&payload), Some(previous));
                    let write = AtomicWrite::builder()
                        .entity_id(Arc::clone(&entity_id)) // clone: Arc ref-count bump; the identifier outlives the attempt
                        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&payload))?)) // clone: Arc ref-count bump
                        .history(history)
                        .subscription_matches(plan.matches)
                        .build();
                    Ok((write, plan.events))
                })
                .await?;

            for event in events {
                self.notifier.publish(target, event);
            }
            Ok(())
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

/// The replacement payload, identified as the entity it replaces.
///
/// Clause 5.6.18.4 replaces the target "with the same Entity ID with the new
/// Entity content provided", and the target is named by the request URL. A
/// payload may repeat that `id` or leave it out; either way the stored payload
/// carries it, because every read that returns an entity from a query takes
/// its `id` from the payload. A payload naming another entity contradicts the
/// URL and is refused rather than silently rewritten.
fn identified_as(mut expanded: Value, target: &Identifier) -> Result<Value, EntityError> {
    match extract_id(&expanded) {
        Ok(named) if named != *target => {
            return Err(EntityError::Validation(EntityValidationReason::ReplacementNamesAnotherEntity { named }));
        }
        Ok(_) | Err(ExpandedMemberError::MissingId) => {}
        Err(
            error @ (ExpandedMemberError::Id { .. }
            | ExpandedMemberError::TypeNotString
            | ExpandedMemberError::Type { .. }
            | ExpandedMemberError::AttributeName { .. }),
        ) => return Err(EntityError::Validation(EntityValidationReason::ExpandedMember(error))),
    }
    // JSON-LD expansion drops a top-level node that carries nothing but its
    // `@id`, so a payload holding only an `id` expands to no node at all. It is
    // left as it is for the entity validation that follows to refuse, the
    // clause 6.4.3.3 `BadRequestData` of a replacement with no content.
    if let Some(node) = expanded_object_mut(&mut expanded) {
        node.insert("@id".to_owned(), Value::String(target.to_string()));
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use crate::{entity_validation_reason::EntityValidationReason, error::EntityError, mutation::replace::identified_as};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use serde_json::json;

    fn target() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI")
    }

    /// A replacement that leaves out its `id` is stored under the target's, so
    /// a query that reads the entity back finds its identifier.
    #[test]
    fn a_replacement_without_an_id_takes_the_targets() {
        let replaced = identified_as(json!([{"@type": ["https://example.org/Vehicle"]}]), &target()).expect("admitted");
        pretty_assertions::assert_eq!(replaced[0]["@id"], "urn:ngsi-ld:Vehicle:1");
    }

    #[test]
    fn a_replacement_repeating_the_targets_id_keeps_it() {
        let replaced = identified_as(json!([{"@id": "urn:ngsi-ld:Vehicle:1"}]), &target()).expect("admitted");
        pretty_assertions::assert_eq!(replaced[0]["@id"], "urn:ngsi-ld:Vehicle:1");
    }

    /// A payload holding only an `id` expands to no node; it passes through
    /// untouched so the entity validation refuses it as a bad request rather
    /// than it failing here as an internal error.
    #[test]
    fn a_replacement_that_expands_to_no_node_is_left_for_validation() {
        let replaced = identified_as(json!([]), &target()).expect("left for validation");
        pretty_assertions::assert_eq!(replaced, json!([]));
    }

    /// Clause 5.6.18.4 keeps "the same Entity ID", so a payload naming another
    /// entity contradicts the request and is refused.
    #[test]
    fn a_replacement_naming_another_entity_is_refused() {
        let refused = identified_as(json!([{"@id": "urn:ngsi-ld:Vehicle:2"}]), &target()).expect_err("refused");
        assert!(
            matches!(refused, EntityError::Validation(EntityValidationReason::ReplacementNamesAnotherEntity { ref named }) if named.as_str() == "urn:ngsi-ld:Vehicle:2"),
            "{refused:?}"
        );
    }
}
