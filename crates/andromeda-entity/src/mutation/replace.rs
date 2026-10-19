//! Replace Entity (clause 5.6.18).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    write_kind::ReplaceEntity,
    write_target::WriteTarget,
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, EntityDocumentParts},
    entity_state::EntityState,
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use std::sync::Arc;

impl EntityService {
    /// Replace an entire entity with its admitted payload (NGSI-LD 5.6.18).
    ///
    /// The replacement drops every attribute it omits, so each dropped instance
    /// needs a tombstone, and that diff is only meaningful against the state
    /// the entity actually had when the write landed. The write therefore
    /// states the `modifiedAt` its diff assumed; a writer that got in first
    /// makes the store refuse it, and the entity is read again.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::NotFound`] when no entity carries the identifier
    /// of `entity` or the type selector of `entity` excludes it (clause
    /// 5.6.18.4), [`EntityError::Validation`] when the written document is no entity the
    /// replacement can store or names an entity other than `id`,
    /// [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn replace_entity(&self, target: &ContextTarget, entity: WriteTarget<'_>, admitted: AdmittedEntity<ReplaceEntity>) -> Result<(), EntityError> {
        let id = entity.id;
        let timer = entity::start(EntityOperation::Replace);
        let result: Result<(), EntityError> = async {
            if self.reader.get_snapshot(target, id).await?.is_none() {
                return Err(EntityError::not_found(id));
            }
            let mut replacement = Arc::new(EntityState::try_from(identified_as(admitted.into_document(), id)?)?);
            let now = SystemInstant::new(Utc::now());

            let events = self
                .commit_computed_write(target, entity, |previous| {
                    // Clause 4.8: an attribute the replacement keeps (same datasetId)
                    // keeps its createdAt, matching the Entity-level createdAt column
                    // that a replace does not rewrite; an instance whose content
                    // changed takes `now` for modifiedAt, one restated unchanged
                    // keeps its own. Stamped per attempt against that attempt's
                    // stored state, which overwrites every stamp an earlier attempt
                    // left; the replacement is copied only while an earlier
                    // attempt's write still shares it.
                    Arc::make_mut(&mut replacement).settle(Some(previous), now);
                    let history = mutation_history(previous, &replacement);
                    // clone: Arc ref-count bump; the write stores the replacement
                    self.write_if_modified(target, id, previous, Arc::clone(&replacement), history)
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

/// The replacement document, identified as the entity it replaces.
///
/// Clause 5.6.18.4 replaces the target "with the same Entity ID with the new
/// Entity content provided", and the target is named by the request URL. A
/// payload may repeat that `id` or leave it out; either way the stored state
/// carries it, because every read that returns an entity from a query takes
/// its `id` from the state. A payload naming another entity contradicts the
/// URL and is refused rather than silently rewritten.
fn identified_as(document: EntityDocument, target: &Identifier) -> Result<EntityDocument, EntityError> {
    let EntityDocumentParts {
        id,
        types,
        scope,
        expires_at,
        attributes,
    } = document.into_parts();
    match id {
        Some(named) if named != *target => Err(EntityError::Validation(EntityValidationReason::ReplacementNamesAnotherEntity { named })),
        Some(_) | None => Ok(EntityDocument::new(
            Some(target.clone()), // clone: the stored state owns the identifier the URL names
            types,
            scope,
            expires_at,
            attributes,
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_validation_reason::EntityValidationReason, error::EntityError, mutation::replace::identified_as};
    use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use serde_json::{Value, json};

    fn target() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI")
    }

    fn document(expanded: &Value) -> EntityDocument {
        EntityDocument::from_slice(&serde_json::to_vec(expanded).expect("JSON")).expect("a written document")
    }

    /// A replacement that leaves out its `id` is stored under the target's, so
    /// a query that reads the entity back finds its identifier.
    #[test]
    fn a_replacement_without_an_id_takes_the_targets() {
        let replaced = identified_as(document(&json!([{"@type": ["https://example.org/Vehicle"]}])), &target()).expect("admitted");
        pretty_assertions::assert_eq!(replaced.id(), Some(&target()));
    }

    #[test]
    fn a_replacement_repeating_the_targets_id_keeps_it() {
        let replaced = identified_as(
            document(&json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.org/Vehicle"]}])),
            &target(),
        )
        .expect("admitted");
        pretty_assertions::assert_eq!(replaced.id(), Some(&target()));
    }

    /// Clause 6.4.3.3: a replacement with no content is a bad request; it is
    /// identified like any other and refused when it becomes the state.
    #[test]
    fn a_replacement_without_content_is_refused_as_a_bad_request() {
        let replaced = identified_as(EntityDocument::default(), &target()).expect("identified");
        let refused = EntityState::try_from(replaced).map_err(EntityError::from).expect_err("no type, no entity");
        assert!(matches!(refused, EntityError::Validation(_)), "{refused:?}");
    }

    /// Clause 5.6.18.4 keeps "the same Entity ID", so a payload naming another
    /// entity contradicts the request and is refused.
    #[test]
    fn a_replacement_naming_another_entity_is_refused() {
        let refused = identified_as(
            document(&json!([{"@id": "urn:ngsi-ld:Vehicle:2", "@type": ["https://example.org/Vehicle"]}])),
            &target(),
        )
        .expect_err("refused");
        assert!(
            matches!(refused, EntityError::Validation(EntityValidationReason::ReplacementNamesAnotherEntity { ref named }) if named.as_str() == "urn:ngsi-ld:Vehicle:2"),
            "{refused:?}"
        );
    }
}
