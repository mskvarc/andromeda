//! Deleting an attribute from an entity (clause 5.6.5).

use crate::{
    EntityService,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    modification_write::ModificationWrite,
    read_modify_write::ComputedLanding,
    write_target::WriteTarget,
};
use andromeda_context_commit::history_record::{HistoryRecord, tombstone};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    operation::{mutation_error::MutationError, selection::DatasetSelection},
};
use andromeda_ngsi_ld_expansion::prefixed_names::path_attribute;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use std::sync::Arc;

impl EntityService {
    /// The expanded name of the attribute `attr_name` names under `context`,
    /// which is the attribute a deletion removes.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `attr_name` cannot be expanded
    /// against `context`, and [`EntityError::Validation`] when `context`
    /// reads it as a prefixed name that breaks the clause 4.6.2 grammar after
    /// its colon, or its expansion is not an attribute name.
    pub async fn expand_attribute_name(&self, attr_name: &AttributeName, context: &ContextSpec) -> Result<AttributeName, EntityError> {
        let prepared_context = self.engine.prepare_term_context(context);
        path_attribute(prepared_context.expansion(&self.engine).await?, attr_name)?;
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;
        attribute_name_of(attr_iri.as_str())
    }

    /// Delete one attribute, or one of its instances, from an entity
    /// (clause 5.6.5).
    ///
    /// The deletion is computed from a state read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `attr_name` cannot be expanded
    /// against `context`, [`EntityError::Validation`] when its expansion is
    /// not an attribute name, [`EntityError::NotFound`] when no entity carries
    /// the identifier of `entity` or the type selector of `entity` excludes it
    /// (clause 5.6.5.4),[`EntityError::MemberNotFound`] when the entity carries neither
    /// the attribute nor the selected instance, [`EntityError::Contention`]
    /// when concurrent writers superseded the computed write on every attempt,
    /// and [`EntityError::Persistence`] when the store could not apply it.
    pub async fn delete_attr(
        &self,
        target: &ContextTarget,
        entity: WriteTarget<'_>,
        attr_name: &AttributeName,
        selection: DatasetSelection<'_>,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let id = entity.id;
        let attribute = self.expand_attribute_name(attr_name, context).await?;

        // Resolve target dataset ids for tombstones before mutating the entity
        // state. The catalogue outlives the attributes it describes, so this
        // does not have to be re-read per attempt.
        let tombstone_targets: Vec<Option<Identifier>> = match selection {
            DatasetSelection::All => match self.attribute_history.dataset_identifiers(target, id, &attribute).await {
                Ok(dataset_identifiers) => dataset_identifiers,
                Err(e) => {
                    tracing::warn!(error = %e, entity_id = %id, event = "tea_lookup_failed_for_deletion");
                    Vec::new()
                }
            },
            DatasetSelection::Single(dataset_id) => vec![Some(dataset_id.clone())], // clone: the tombstone target survives the borrowed selection.
            DatasetSelection::Default => vec![None],
        };

        let ComputedLanding { value: events, committed_at } = self
            .commit_computed_write(target, entity, |read, now| {
                let previous = &read.state;
                let mut remaining = EntityState::clone(previous); // clone: shares every attribute the deletion leaves alone
                delete_member(&mut remaining, &attribute, selection)?;

                let history = deletion_history(previous, &attribute, &tombstone_targets);

                let ModificationWrite { write, events } = self.modification_write(target, id, read, Arc::new(remaining), history, now)?;
                Ok((Some(write), events))
            })
            .await?;

        if let Some(committed_at) = committed_at {
            self.notifier.publish(target, committed_at, events).await;
        }

        Ok(())
    }
}

/// Read an expanded attribute IRI as the attribute name it stands for.
///
/// The IRI comes straight out of JSON-LD term expansion, so a value that fails
/// the NGSI-LD name grammar means the caller asked for something that cannot
/// name an attribute at all.
fn attribute_name_of(iri: &str) -> Result<AttributeName, EntityError> {
    iri.parse()
        .map_err(|source| EntityError::Validation(EntityValidationReason::AttributeName(source)))
}

/// What deleting `attribute` from `previous` records in the Temporal
/// Evolution: one tombstone per instance in `tombstone_targets`.
///
/// Clause 5.6.5.4 handles the Scope before the `deleteAll` and `datasetId`
/// bullets, "If the target Attribute is scope, remove the scope Attribute
/// from the target Entity", so the removal is unconditional, and clause 4.18
/// ends the Scope's validity at that delete operation. The Scope has no
/// attribute stream to tombstone: clause 4.5.4 gives it no dataset dimension
/// and no attribute family, so its history is the scope removal and nothing
/// else. Clause 4.22 makes the entity `expiresAt` a system Property of the
/// storage, not an Attribute, so its removal records no history at all.
fn deletion_history(previous: &EntityState, attribute: &AttributeName, tombstone_targets: &[Option<Identifier>]) -> Vec<HistoryRecord> {
    let name = attribute.as_str();
    if name == expanded_properties::SCOPE.as_str() {
        return vec![HistoryRecord::ScopeRemoval];
    }
    if name == expanded_properties::EXPIRES_AT.as_str() {
        return Vec::new();
    }
    tombstone_targets
        .iter()
        .map(|target| {
            // The family is read from the state the deletion is computed
            // against, so the tombstone renders the null member its family
            // prescribes (clauses 4.5.7 and 4.5.8). A target the catalogue
            // knows and the state does not is a Property by clause 4.5.2.
            let attribute_type = previous.attribute_instance_family(name, target.as_ref()).unwrap_or(AttributeType::Property);
            tombstone(attribute, target.as_ref(), attribute_type)
        })
        .collect()
}

/// Delete the member `attribute` names from `state` (clause 5.6.5).
///
/// `scope` (clause 4.18) and the entity `expiresAt` (clause 4.22) are named
/// like attributes but hold no instances: the whole member goes unless a
/// `datasetId` is selected, which neither of them carries.
fn delete_member(state: &mut EntityState, attribute: &AttributeName, selection: DatasetSelection<'_>) -> Result<(), MutationError> {
    let name = attribute.as_str();
    let held = if name == expanded_properties::SCOPE.as_str() {
        state.scope().is_some()
    } else if name == expanded_properties::EXPIRES_AT.as_str() {
        state.expires_at().is_some()
    } else {
        return state.delete_attribute(attribute, selection);
    };
    if !held {
        return Err(MutationError::AttributeNotFound {
            attribute: attribute.clone(), // clone: the error outlives the borrowed request
        });
    }
    match selection {
        DatasetSelection::Single(dataset_id) => Err(MutationError::missing_instance(attribute, Some(dataset_id))),
        DatasetSelection::All | DatasetSelection::Default => {
            if name == expanded_properties::SCOPE.as_str() {
                state.set_scope(None);
            } else {
                state.set_expires_at(None);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attrs::delete::{delete_member, deletion_history};
    use andromeda_context_commit::history_record::HistoryRecord;
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        operation::{mutation_error::MutationError, selection::DatasetSelection},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use serde_json::json;

    fn scoped() -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/a"}],
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity")
    }

    fn name(iri: &str) -> AttributeName {
        iri.parse().expect("a valid attribute name")
    }

    /// Clause 5.6.5.4: "If the target Attribute is scope, remove the scope
    /// Attribute from the target Entity".
    #[test]
    fn deleting_the_scope_removes_it() {
        let mut state = scoped();
        delete_member(&mut state, &name("https://uri.etsi.org/ngsi-ld/scope"), DatasetSelection::All).expect("the scope is deleted");
        assert!(state.scope().is_none());
        assert!(state.attributes().contains("https://example.org/speed"));
    }

    /// The scope carries no `datasetId`, so no dataset selects it.
    #[test]
    fn a_dataset_of_the_scope_is_not_found() {
        let mut state = scoped();
        let dataset: Identifier = "urn:ngsi-ld:Dataset:a".parse().expect("a valid identifier");
        let error =
            delete_member(&mut state, &name("https://uri.etsi.org/ngsi-ld/scope"), DatasetSelection::Single(&dataset)).expect_err("the scope has no dataset");
        assert!(matches!(error, MutationError::DatasetNotFound { .. }), "{error:?}");
        assert!(state.scope().is_some());
    }

    #[test]
    fn deleting_an_absent_expiry_is_not_found() {
        let mut state = scoped();
        let error = delete_member(&mut state, &name("https://uri.etsi.org/ngsi-ld/expiresAt"), DatasetSelection::All).expect_err("no expiry is held");
        assert!(matches!(error, MutationError::AttributeNotFound { .. }), "{error:?}");
    }

    #[test]
    fn an_attribute_is_deleted_by_its_name() {
        let mut state = scoped();
        delete_member(&mut state, &name("https://example.org/speed"), DatasetSelection::All).expect("the attribute is deleted");
        assert!(!state.attributes().contains("https://example.org/speed"));
        assert!(state.scope().is_some());
    }

    /// Clause 4.22: the entity `expiresAt` is a system Property of the
    /// storage with no Temporal Evolution.
    #[test]
    fn deleting_the_expiry_records_no_history() {
        let history = deletion_history(&scoped(), &name("https://uri.etsi.org/ngsi-ld/expiresAt"), &[None]);
        assert!(history.is_empty(), "{history:?}");
    }

    #[test]
    fn deleting_the_scope_records_its_removal() {
        let history = deletion_history(&scoped(), &name("https://uri.etsi.org/ngsi-ld/scope"), &[None]);
        assert!(matches!(history.as_slice(), [HistoryRecord::ScopeRemoval]), "{history:?}");
    }

    #[test]
    fn deleting_an_attribute_tombstones_each_target() {
        let history = deletion_history(&scoped(), &name("https://example.org/speed"), &[None]);
        assert!(matches!(history.as_slice(), [HistoryRecord::Tombstone(_)]), "{history:?}");
    }
}
