//! Purging attributes from a set of entities (clause 5.6.21), whole or
//! scoped to a `datasetId` (the Andromeda `purge_dataset_scope` extension).
//!
//! Each entity is its own atomic write: the state rewrite and the tombstones
//! for the purged instances commit together, and one entity failing must not
//! undo the ones already purged. The diff is computed from a state read
//! outside that write, so the write states the `modifiedAt` it assumed and is
//! recomputed against a fresh read if a concurrent writer got in first.

use crate::{EntityService, entity_validation_reason::EntityValidationReason, error::EntityError, modification_write::ModificationWrite};
use andromeda_context_commit::{
    change_event::ChangeEvent,
    history_record::{HistoryRecord, removal_history},
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    operation::{
        purge::{AttributePurge, PurgeOutcome},
        selection::DatasetSelection,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    projection::purge::{DatasetPurgeScope, PurgeMode},
};
use std::sync::Arc;

impl EntityService {
    /// Purge attributes from entities according to the given mode (clause 5.6.21).
    ///
    /// - `Drop`: remove the listed attributes from each entity
    /// - `Keep`: retain only the listed attributes, removing all others
    ///
    /// A `datasetId` selector removes only the instances it names of each
    /// selected attribute; `deleteAll`, like no selector, removes every
    /// instance, which is the whole attribute.
    ///
    /// Returns the number of entities modified.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when one of the named attributes cannot
    /// be expanded against `context`, [`EntityError::Validation`] when its
    /// expansion is not an attribute name, [`EntityError::Contention`] when
    /// concurrent writers superseded an entity's purge on every attempt, and
    /// [`EntityError::Persistence`] when the store could not read an entity or
    /// commit its purge.
    pub async fn purge_attrs(&self, target: &ContextTarget, entity_ids: &[Identifier], mode: &PurgeMode, context: &ContextSpec) -> Result<u32, EntityError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let attributes: Vec<AttributeName> = match mode {
            PurgeMode::DeleteEntities => return Ok(0),
            PurgeMode::Drop { attributes, .. } | PurgeMode::Keep { attributes, .. } => {
                let mut expanded = Vec::with_capacity(attributes.len());
                for attribute in attributes {
                    let iri = self.engine.expand_term(attribute.as_str(), &prepared_context).await?;
                    expanded.push(
                        iri.as_str()
                            .parse()
                            .map_err(|source| EntityError::Validation(EntityValidationReason::AttributeName(source)))?,
                    );
                }
                expanded
            }
        };

        let mut modified_count = 0u32;

        for entity_id in entity_ids {
            let mut purged: Option<Vec<ChangeEvent>> = None;
            let outcome = self
                .commit_read_modify_write(target, entity_id, |read, now| {
                    let previous = &read.state;
                    let Some((current, history)) = purged_state(previous, mode, &attributes) else {
                        return Ok(None);
                    };
                    let ModificationWrite { write, events } = self.modification_write(target, entity_id, read, Arc::new(current), history, now)?;
                    purged = Some(events);
                    Ok(Some(write))
                })
                .await?;

            let (Some(committed_at), Some(events)) = (outcome.as_ref().and_then(WriteOutcome::changed_at), purged) else {
                continue;
            };
            modified_count += 1;

            // A purged attribute is a deleted attribute, and clause 5.2.12's
            // `attributeDeleted` trigger says what kind of change fires a
            // notification rather than which operation caused it. Reported the
            // way clause 5.6.5 reports the single-attribute form.
            self.notifier.publish(target, committed_at, events).await;
        }

        Ok(modified_count)
    }
}

/// What `previous` becomes once purged as `mode` says, with a tombstone for
/// every instance it lost, or `None` when it holds nothing the purge selects.
fn purged_state(previous: &EntityState, mode: &PurgeMode, attributes: &[AttributeName]) -> Option<(EntityState, Vec<HistoryRecord>)> {
    let purge = match mode {
        PurgeMode::DeleteEntities => return None,
        PurgeMode::Drop { .. } => AttributePurge::Drop(attributes),
        PurgeMode::Keep { .. } => AttributePurge::Keep(attributes),
    };
    let mut purged = EntityState::clone(previous);
    let outcome = match mode.dataset_scope()? {
        // `deleteAll` removes every instance, which is the whole attribute, so
        // it is the standard whole-attribute purge; only a `datasetId`
        // selector removes single instances, through the clause 5.6.5 rule.
        DatasetPurgeScope::Unfiltered | DatasetPurgeScope::All => purged.purge_attributes(purge),
        DatasetPurgeScope::Selector(selector) => purged.purge_dataset_instances(purge, selection_of(selector)),
    };
    match outcome {
        PurgeOutcome::Untouched => None,
        PurgeOutcome::Purged => {
            let history = removal_history(previous, &purged);
            Some((purged, history))
        }
    }
}

/// The clause 5.6.5 selection a `datasetId` selector lowers to: `@none`
/// selects the default instance, a URI that one dataset.
const fn selection_of(selector: &DatasetSelector) -> DatasetSelection<'_> {
    match selector {
        DatasetSelector::Default => DatasetSelection::Default,
        DatasetSelector::Id(dataset_id) => DatasetSelection::Single(dataset_id),
    }
}

#[cfg(test)]
mod tests {
    use crate::purge::purged_state;
    use andromeda_context_commit::history_record::HistoryRecord;
    use andromeda_ngsi_ld_entity::{attribute_instances::AttributeInstances, entity_state::EntityState};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        projection::purge::{DatasetPurgeScope, PurgeMode},
    };
    use serde_json::json;

    /// A Vehicle whose `speed` has a default and a `datasetId` instance and
    /// whose `brand` has one.
    fn vehicle() -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            "https://example.org/speed": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]}
            ],
            "https://example.org/brand": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "x"}]}]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity")
    }

    fn speed() -> Vec<AttributeName> {
        vec!["https://example.org/speed".parse().expect("a valid attribute name")]
    }

    fn drop(scope: DatasetPurgeScope) -> PurgeMode {
        PurgeMode::Drop {
            attributes: speed(),
            dataset: scope,
        }
    }

    /// Clause 5.6.21 `drop`: the listed attribute goes whole, one tombstone
    /// per instance it held.
    #[test]
    fn a_whole_attribute_purge_tombstones_every_instance() {
        let (purged, history) = purged_state(&vehicle(), &drop(DatasetPurgeScope::Unfiltered), &speed()).expect("speed is purged");
        assert!(!purged.attributes().contains("https://example.org/speed"));
        assert!(purged.attributes().contains("https://example.org/brand"));
        pretty_assertions::assert_eq!(history.len(), 2);
        assert!(history.iter().all(|record| matches!(record, HistoryRecord::Tombstone(_))));
    }

    /// A `datasetId` selector removes only the instance it names.
    #[test]
    fn a_dataset_purge_tombstones_only_the_selected_instance() {
        let selector = DatasetSelector::Id("urn:ngsi-ld:Dataset:a".parse().expect("a valid identifier"));
        let (purged, history) = purged_state(&vehicle(), &drop(DatasetPurgeScope::Selector(selector)), &speed()).expect("one instance is purged");
        pretty_assertions::assert_eq!(purged.attributes().get("https://example.org/speed").map(AttributeInstances::len), Some(1));
        pretty_assertions::assert_eq!(history.len(), 1);
    }

    /// Clause 4.18 ends a Scope's validity at the operation that removes it,
    /// so a purge naming `scope` records the Scope's removal.
    #[test]
    fn purging_the_scope_records_its_removal() {
        let previous = EntityState::from_slice(
            &serde_json::to_vec(&json!([{
                "@id": "urn:ngsi-ld:Vehicle:1",
                "@type": ["https://example.org/Vehicle"],
                "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/a/b"}]
            }]))
            .expect("JSON"),
        )
        .expect("a stored entity");
        let scope: Vec<AttributeName> = vec!["https://uri.etsi.org/ngsi-ld/scope".parse().expect("a valid attribute name")];
        let mode = PurgeMode::Drop {
            attributes: scope.clone(),
            dataset: DatasetPurgeScope::Unfiltered,
        };
        let (purged, history) = purged_state(&previous, &mode, &scope).expect("the scope is purged");
        assert!(purged.scope().is_none());
        assert!(matches!(history.as_slice(), [HistoryRecord::ScopeRemoval]), "{history:?}");
    }

    #[test]
    fn a_purge_selecting_nothing_writes_nothing() {
        let selector = DatasetSelector::Id("urn:ngsi-ld:Dataset:z".parse().expect("a valid identifier"));
        assert!(purged_state(&vehicle(), &drop(DatasetPurgeScope::Selector(selector)), &speed()).is_none());
        assert!(purged_state(&vehicle(), &PurgeMode::DeleteEntities, &speed()).is_none());
    }
}
