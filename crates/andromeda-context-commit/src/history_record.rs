//! Temporal Evolution consequences carried by an atomic write.

use crate::scope_presence::ScopePresence;
use andromeda_ngsi_ld_entity::{
    document::EntityDocument,
    entity_state::EntityState,
    history::written_observation::{AddressedInstances, written_observation},
    reads::{
        attribute_instance_key::AttributeInstanceKey,
        removed_instance::{RemovedAttributeInstance, removed_attribute_instances},
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use derive_more::IsVariant;
use std::sync::Arc;

/// One temporal-history consequence of an entity mutation.
#[derive(Debug, IsVariant)]
pub enum HistoryRecord {
    /// Attribute observations of an entity, recorded because a Core write
    /// changed the Entity's current state (clause 4.3.1: "Brokers that intend
    /// to implement the Temporal API should consider updating the Temporal
    /// Evolution of an Entity whenever the 'current state' is modified via the
    /// Core API").
    Observations(Arc<EntityState>),
    /// Attribute instances added to the Temporal Evolution of an Entity through
    /// the Temporal API (clauses 5.6.11 and 5.6.12), recorded exactly as the
    /// client sent them.
    ///
    /// Clauses 5.6.11.4 and 5.6.12.4 add the instances "to the target Temporal
    /// Evolution of an Entity", so every one of them is history. What they do
    /// to current state is a separate question -- current state holds one
    /// instance per Attribute instance (clause 4.5.5), the most current one
    /// (clause 4.5.5.3) -- which the write answers through its
    /// [`CurrentStateProjection`](crate::current_state_projection::CurrentStateProjection)
    /// rather than through its history.
    EvolutionInstances(Arc<EntityDocument>),
    /// End of one attribute instance that left current entity state.
    Tombstone(RemovedAttributeInstance),
    /// End of the Entity's Scope, which left current entity state.
    ///
    /// Clause 4.18: "For the Temporal Evolution a given Scope is considered
    /// valid from the time it has been set until the time it has been
    /// explicitly removed by an update or delete operation." A write that
    /// leaves the Entity with no Scope is such an operation — a Replace Entity
    /// (clause 5.6.18.4: "completely replace the existing Entity ... with the
    /// new Entity content provided") that omits `scope`, or the deletion of the
    /// Entity itself — and without this record the reconstructed interval would
    /// stay open for ever and report a Scope the Entity has not been in since.
    ///
    /// It carries nothing, because clause 4.18 gives an Entity at most one
    /// instance of the scope Property: there is only ever one Scope value to
    /// end, and `chk_scope_history_value_presence` forbids the stored removal
    /// from naming it.
    ScopeRemoval,
}

/// Build a tombstone for one validated attribute-instance identity.
///
/// The family travels with the record because a deletion renders the
/// `urn:ngsi-ld:null` member of the family the instance belonged to (clauses
/// 4.5.7 and 4.5.8), and current state no longer holds the instance to ask.
#[must_use]
pub fn tombstone(attr: &AttributeName, dataset_id: Option<&Identifier>, attribute_type: AttributeType) -> HistoryRecord {
    HistoryRecord::Tombstone(RemovedAttributeInstance {
        key: AttributeInstanceKey {
            attr: attr.clone(),              // clone: the history record owns its attribute name.
            dataset_id: dataset_id.cloned(), // clone: the history record owns its dataset identifier.
        },
        attribute_type,
    })
}

/// Build the observations and exact instance tombstones produced by a state
/// mutation.
///
/// `previous` is the Entity before the write and `current` the Entity it
/// stored. The observation is the [`written_observation`] of the instances
/// `addressed` names: each as `current` holds it, never the request Fragment
/// that produced it. Clauses 4.5.7 and 4.5.8 make every entry of the Temporal
/// Evolution an instance "as mandated by clause 4.5.2", and a Fragment is only
/// the part of one a request restated -- recording it would lose a `unitCode`
/// clause 5.5.8 EXAMPLE 1 keeps, the value members clause 5.5.12 merges into,
/// and the NGSI-LD Null deletion instructions would read as values. No
/// observation is recorded when the write applied no instance and no Scope.
///
/// A Scope the mutation left behind is recorded too. Clause 4.18 keeps a Scope
/// valid "until the time it has been explicitly removed by an update or delete
/// operation", and a Replace Entity that omits `scope` is such an operation:
/// clause 5.6.18.4 replaces the Entity with the content provided, so the Scope
/// is gone from current state and its temporal interval has to close with it.
/// A mutation that names a different Scope needs no record of its own — the
/// event it writes is itself what ends the interval before it.
#[must_use]
pub fn mutation_history(addressed: AddressedInstances<'_>, previous: &EntityState, current: &EntityState) -> Vec<HistoryRecord> {
    let removed = removed_attribute_instances(previous, current);
    let observation = written_observation(addressed, Some(previous), current);
    let mut history = Vec::with_capacity(removed.len() + 2);
    history.extend(observation.map(|observed| HistoryRecord::Observations(Arc::new(observed))));
    history.extend(removed.into_iter().map(HistoryRecord::Tombstone));
    if previous.scope().is_some() && current.scope().is_none() {
        history.push(HistoryRecord::ScopeRemoval);
    }
    history
}

/// Build the tombstone every attribute instance of a deleted entity needs.
///
/// Clause 5.6.6 deletes the entity's current state, and clause 4.5.8 has an
/// instance that has ended carry a `deletedAt`. The deletion therefore records
/// one tombstone per instance the entity still held, so temporal history says
/// when each instance ended instead of trailing off with no explanation.
#[must_use]
pub fn deletion_history(previous: &EntityState) -> Vec<HistoryRecord> {
    let instances = previous.instance_families().map(|(attr, dataset_id, attribute_type)| {
        (
            AttributeInstanceKey {
                attr: attr.clone(),              // clone: the tombstone owns its attribute name.
                dataset_id: dataset_id.cloned(), // clone: the tombstone owns its dataset identifier.
            },
            attribute_type,
        )
    });
    let scope = match previous.scope() {
        Some(_) => ScopePresence::Held,
        None => ScopePresence::Absent,
    };
    deletion_tombstones(instances, scope)
}

/// The tombstones a deleted entity's history records, from the instances it
/// held and whether it held a Scope; [`deletion_history`] reads both off the
/// removed state, and a deletion that never read the state states them
/// itself.
///
/// The order is fixed by the instance identity rather than left to the set that
/// collected it: a tombstone's position in the write is half of the key its
/// stored row deduplicates on, so a replay has to produce the same order.
pub fn deletion_tombstones(instances: impl IntoIterator<Item = (AttributeInstanceKey, AttributeType)>, scope: ScopePresence) -> Vec<HistoryRecord> {
    let mut removed = instances
        .into_iter()
        .map(|(key, attribute_type)| RemovedAttributeInstance { key, attribute_type })
        .collect::<Vec<RemovedAttributeInstance>>();
    removed.sort_by(|left, right| {
        left.key.attr.as_str().cmp(right.key.attr.as_str()).then_with(|| {
            left.key
                .dataset_id
                .as_ref()
                .map(Identifier::as_str)
                .cmp(&right.key.dataset_id.as_ref().map(Identifier::as_str))
        })
    });
    let mut history: Vec<HistoryRecord> = removed.into_iter().map(HistoryRecord::Tombstone).collect();
    // Clause 4.18 ends a Scope's validity at the "delete operation" as well as
    // at an update, so an Entity that still held a Scope closes its interval
    // here rather than leaving it open past the Entity itself.
    match scope {
        ScopePresence::Held => history.push(HistoryRecord::ScopeRemoval),
        ScopePresence::Absent => {}
    }
    history
}

#[cfg(test)]
mod tests {
    use crate::{
        history_record::{HistoryRecord, deletion_history, deletion_tombstones, mutation_history},
        scope_presence::ScopePresence,
    };
    use andromeda_ngsi_ld_entity::{
        document::{EntityDocument, instance_document::InstanceDocument},
        entity_state::EntityState,
        history::written_observation::AddressedInstances,
        operation::selection::OverwritePolicy,
        reads::{attribute_instance_key::AttributeInstanceKey, removed_instance::RemovedAttributeInstance},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
    use serde_json::{Value, json};

    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";

    /// Decode a stored document into a state.
    fn state(document: &Value) -> EntityState {
        EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes")
    }

    /// Decode a written entity fragment.
    fn fragment(document: &Value) -> EntityDocument {
        EntityDocument::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a written fragment decodes")
    }

    /// An entity of the given members.
    fn entity(members: &Value) -> Value {
        let mut node = json!({"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/T"]});
        if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
            node.extend(members.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        json!([node])
    }

    fn property(value: i64) -> Value {
        json!({"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]})
    }

    #[test]
    fn a_replacement_tombstones_a_dropped_attribute() {
        let previous = state(&entity(
            &json!({"https://example.org/speed": [property(1)], "https://example.org/colour": [property(2)]}),
        ));
        let replacement_document = entity(&json!({"https://example.org/speed": [property(1)]}));
        let replacement = state(&replacement_document);
        let written = fragment(&replacement_document);
        let addressed = AddressedInstances::EntityFragment {
            fragment: &written,
            overwrite: OverwritePolicy::Overwrite,
        };
        let history = mutation_history(addressed, &previous, &replacement);
        assert!(history[0].is_observations());
        assert!(history.iter().any(HistoryRecord::is_tombstone));
    }

    #[test]
    fn mutation_history_tombstones_the_exact_removed_dataset_instance() {
        let dataset_a = "urn:ngsi-ld:Dataset:a";
        let dataset_b = "urn:ngsi-ld:Dataset:b";
        let instance = |dataset_id: &str| {
            json!({
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
            })
        };
        let previous = state(&entity(&json!({"https://example.com/speed": [instance(dataset_a), instance(dataset_b)]})));
        let current = state(&entity(&json!({"https://example.com/speed": [instance(dataset_b)]})));
        let deletion = fragment(&json!([{
            "https://example.com/speed": [{
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_a}]
            }]
        }]));
        let addressed = AddressedInstances::EntityFragment {
            fragment: &deletion,
            overwrite: OverwritePolicy::Overwrite,
        };

        let history = mutation_history(addressed, &previous, &current);

        // The deletion is recorded by its tombstone alone: the NGSI-LD Null is
        // an instruction, and the instance it removed is not in current state.
        pretty_assertions::assert_eq!(history.len(), 1);
        let Some(HistoryRecord::Tombstone(removed)) = history.first() else {
            panic!("the only record is the tombstone: {history:?}");
        };
        pretty_assertions::assert_eq!(removed.key.attr.as_str(), "https://example.com/speed");
        pretty_assertions::assert_eq!(removed.key.dataset_id.as_ref().map(Identifier::as_str), Some(dataset_a));
    }

    /// Clause 5.5.8 EXAMPLE 1: a partial update keeps the `unitCode` the
    /// Fragment left out, so the recorded observation is the resulting instance
    /// carrying it rather than the Fragment.
    #[test]
    fn a_partial_update_records_the_resulting_instance_rather_than_the_fragment() {
        let temperature = |value: i64| {
            json!({
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}]
            })
        };
        let previous = state(&entity(&json!({"https://example.org/temperature": [temperature(25)]})));
        let current = state(&entity(&json!({"https://example.org/temperature": [temperature(100)]})));
        let attribute: AttributeName = "https://example.org/temperature".parse().expect("a valid attribute name");
        let written = InstanceDocument::from_slice(
            &attribute,
            &serde_json::to_vec(&json!({"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 100}]})).expect("JSON"),
        )
        .expect("a written attribute fragment decodes");
        let addressed = AddressedInstances::AttributeFragment {
            attribute: &attribute,
            dataset_id: None,
            fragment: &written,
        };

        let history = mutation_history(addressed, &previous, &current);

        pretty_assertions::assert_eq!(history.len(), 1);
        let Some(HistoryRecord::Observations(observed)) = history.first() else {
            panic!("the only record is the observation: {history:?}");
        };
        pretty_assertions::assert_eq!(**observed, current);
    }

    /// A write that applied nothing -- a no-overwrite append of an attribute the
    /// Entity already holds -- records no observation at all.
    #[test]
    fn a_write_that_applied_no_instance_records_no_observation() {
        let held = state(&entity(&json!({"https://example.org/speed": [property(1)]})));
        let append = fragment(&json!([{"https://example.org/speed": [property(2)]}]));
        let addressed = AddressedInstances::EntityFragment {
            fragment: &append,
            overwrite: OverwritePolicy::Preserve,
        };

        let history = mutation_history(addressed, &held, &held);

        assert!(history.is_empty(), "{history:?}");
    }

    /// Clause 4.18: a replacement that omits `scope` ends the Scope's interval.
    #[test]
    fn a_replacement_without_scope_records_the_scope_removal() {
        let previous = state(&entity(&json!({"https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}]})));
        let replacement_document = entity(&json!({}));
        let replacement = state(&replacement_document);
        let written = fragment(&replacement_document);
        let addressed = AddressedInstances::EntityFragment {
            fragment: &written,
            overwrite: OverwritePolicy::Overwrite,
        };

        let history = mutation_history(addressed, &previous, &replacement);

        assert!(matches!(history.as_slice(), [HistoryRecord::ScopeRemoval]), "{history:?}");
    }

    #[test]
    fn deleting_an_entity_tombstones_every_instance_it_still_held_in_a_fixed_order() {
        let instance = |dataset_id: &str| {
            json!({
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
            })
        };
        let previous = state(&entity(&json!({
            "https://example.org/speed": [instance("urn:ngsi-ld:Dataset:b"), instance("urn:ngsi-ld:Dataset:a")],
            "https://example.org/colour": [property(1)]
        })));

        let named = deletion_history(&previous)
            .into_iter()
            .map(|entry| match entry {
                HistoryRecord::Tombstone(removed) => (removed.key.attr.as_str().to_owned(), removed.key.dataset_id.map(|id| id.as_str().to_owned())),
                HistoryRecord::Observations(_) | HistoryRecord::EvolutionInstances(_) | HistoryRecord::ScopeRemoval => (String::new(), None),
            })
            .collect::<Vec<_>>();

        pretty_assertions::assert_eq!(
            named,
            vec![
                ("https://example.org/colour".to_owned(), None),
                ("https://example.org/speed".to_owned(), Some("urn:ngsi-ld:Dataset:a".to_owned())),
                ("https://example.org/speed".to_owned(), Some("urn:ngsi-ld:Dataset:b".to_owned())),
            ]
        );
    }

    #[test]
    fn deleting_an_entity_that_held_no_attributes_records_nothing() {
        assert!(deletion_history(&state(&entity(&json!({})))).is_empty());
    }

    /// What one entry of a deletion's history ends.
    #[derive(Debug, PartialEq)]
    enum Ended {
        /// One attribute instance.
        Instance(RemovedAttributeInstance),
        /// The Entity's Scope.
        Scope,
    }

    /// The entries of a deletion's history as the things they end.
    fn ended(history: Vec<HistoryRecord>) -> Vec<Ended> {
        history
            .into_iter()
            .map(|entry| match entry {
                HistoryRecord::Tombstone(removed) => Ended::Instance(removed),
                HistoryRecord::ScopeRemoval => Ended::Scope,
                HistoryRecord::Observations(_) | HistoryRecord::EvolutionInstances(_) => panic!("a deletion records no observation: {entry:?}"),
            })
            .collect()
    }

    /// An instance identity and its family.
    fn instance(attr: &str, dataset_id: Option<&str>, attribute_type: AttributeType) -> (AttributeInstanceKey, AttributeType) {
        let key = AttributeInstanceKey {
            attr: attr.parse().expect("a valid fixture attribute name"),
            dataset_id: dataset_id.map(|id| id.parse().expect("a valid fixture datasetId")),
        };
        (key, attribute_type)
    }

    /// Clauses 4.5.7, 4.5.8 and 4.18: a deletion that never read the state
    /// states the instances and the Scope it removed itself, and records
    /// exactly the tombstones -- each instance with its family, then the Scope
    /// -- that reading the removed state records.
    #[test]
    fn an_outline_of_every_instance_and_the_scope_records_the_tombstones_the_state_would() {
        let with_dataset = |dataset_id: &str| {
            json!({
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
            })
        };
        let removed = state(&entity(&json!({
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}],
            "https://example.org/speed": [with_dataset("urn:ngsi-ld:Dataset:b"), property(1), with_dataset("urn:ngsi-ld:Dataset:a")],
            "https://example.org/owner": [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:P"}]}]
        })));
        // Listed in an order of its own, as a store answers.
        let outline = vec![
            instance("https://example.org/speed", Some("urn:ngsi-ld:Dataset:a"), AttributeType::Property),
            instance("https://example.org/owner", None, AttributeType::Relationship),
            instance("https://example.org/speed", Some("urn:ngsi-ld:Dataset:b"), AttributeType::Property),
            instance("https://example.org/speed", None, AttributeType::Property),
        ];
        let expected = [
            instance("https://example.org/owner", None, AttributeType::Relationship),
            instance("https://example.org/speed", None, AttributeType::Property),
            instance("https://example.org/speed", Some("urn:ngsi-ld:Dataset:a"), AttributeType::Property),
            instance("https://example.org/speed", Some("urn:ngsi-ld:Dataset:b"), AttributeType::Property),
        ]
        .into_iter()
        .map(|(key, attribute_type)| Ended::Instance(RemovedAttributeInstance { key, attribute_type }))
        .chain([Ended::Scope])
        .collect::<Vec<_>>();

        pretty_assertions::assert_eq!(ended(deletion_tombstones(outline, ScopePresence::Held)), expected);
        pretty_assertions::assert_eq!(ended(deletion_history(&removed)), expected);
    }

    #[test]
    fn an_entity_that_held_no_scope_records_no_scope_removal() {
        let outline = vec![instance("https://example.org/speed", None, AttributeType::Property)];
        assert!(!ended(deletion_tombstones(outline, ScopePresence::Absent)).contains(&Ended::Scope));
    }
}
