//! Temporal Evolution consequences carried by an atomic write.

use crate::{error::CommitError, scope_presence::ScopePresence};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_model::expanded::{
    instance_key::{AttributeInstanceKey, attribute_instance_families},
    removed_instance::{RemovedAttributeInstance, removed_attribute_instances},
    scope_member::extract_scopes,
    written_observation::{AddressedInstances, written_observation},
};
use derive_more::IsVariant;
use serde_json::Value;
use std::sync::Arc;

/// One temporal-history consequence of an entity mutation.
#[derive(Debug, IsVariant)]
pub enum HistoryRecord {
    /// Attribute observations contained in an expanded payload, recorded
    /// because a Core write changed the Entity's current state (clause 4.3.1:
    /// "Brokers that intend to implement the Temporal API should consider
    /// updating the Temporal Evolution of an Entity whenever the 'current
    /// state' is modified via the Core API").
    Observations(Arc<Value>),
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
    EvolutionInstances(Arc<Value>),
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

/// Build the observations and exact instance tombstones produced by a payload mutation.
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
///
/// # Errors
///
/// Returns [`CommitError::Invariant`] when the Fragment or either state
/// carries an instance whose `datasetId` cannot be read, since neither its
/// observation nor its tombstone would name an instance.
pub fn mutation_history(addressed: AddressedInstances<'_>, previous: &Value, current: &Value) -> Result<Vec<HistoryRecord>, CommitError> {
    let removed = removed_attribute_instances(previous, current).map_err(|error| CommitError::Invariant(error.into()))?;
    let observation = written_observation(addressed, previous, current).map_err(|error| CommitError::Invariant(error.into()))?;
    let mut history = Vec::with_capacity(removed.len() + 2);
    history.extend(observation.map(|observed| HistoryRecord::Observations(Arc::new(observed))));
    history.extend(removed.into_iter().map(HistoryRecord::Tombstone));
    if scope_was_cleared(previous, current) {
        history.push(HistoryRecord::ScopeRemoval);
    }
    Ok(history)
}

/// Whether the mutation left the Entity with no Scope at all, having had one.
fn scope_was_cleared(previous: &Value, current: &Value) -> bool {
    !extract_scopes(previous).is_empty() && extract_scopes(current).is_empty()
}

/// Build the tombstone every attribute instance of a deleted entity needs.
///
/// Clause 5.6.6 deletes the entity's current state, and clause 4.5.8 has an
/// instance that has ended carry a
/// `deletedAt`. The deletion therefore records one tombstone per instance the
/// entity still held, so temporal history says when each instance ended instead
/// of trailing off with no explanation.
///
/// The order is fixed by the instance identity rather than left to the set that
/// collected it: a tombstone's position in the write is half of the key its
/// stored row deduplicates on, so a replay has to produce the same order.
///
/// # Errors
///
/// Returns [`CommitError::Invariant`] when the entity carries an instance whose
/// `datasetId` cannot be read.
pub fn deletion_history(previous: &Value) -> Result<Vec<HistoryRecord>, CommitError> {
    let instances = attribute_instance_families(previous).map_err(|error| CommitError::Invariant(error.into()))?;
    let scope = if extract_scopes(previous).is_empty() {
        ScopePresence::Absent
    } else {
        ScopePresence::Held
    };
    Ok(deletion_tombstones(instances, scope))
}

/// The tombstones a deleted entity's history records, from the instances it
/// held and whether it held a Scope; [`deletion_history`] reads both off the
/// removed payload, and a deletion that never read the payload states them
/// itself.
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
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
    use andromeda_ngsi_ld_model::expanded::{
        instance_key::AttributeInstanceKey,
        mutation::selection::OverwritePolicy,
        removed_instance::RemovedAttributeInstance,
        written_observation::AddressedInstances,
    };
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    #[test]
    fn a_replacement_tombstones_a_dropped_attribute() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}],
            "https://example.org/colour": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]
        }]);
        let replacement = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]
        }]);
        let addressed = AddressedInstances::EntityFragment {
            fragment: &replacement,
            overwrite: OverwritePolicy::Overwrite,
        };
        let history = mutation_history(addressed, &previous, &replacement).expect("readable datasetIds");
        assert!(history[0].is_observations());
        assert!(history.iter().any(HistoryRecord::is_tombstone));
    }

    #[test]
    fn mutation_history_tombstones_the_exact_removed_dataset_instance() {
        let dataset_a = "urn:ngsi-ld:Dataset:a";
        let dataset_b = "urn:ngsi-ld:Dataset:b";
        let instance = |dataset_id: &str| {
            json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
            })
        };
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [instance(dataset_a), instance(dataset_b)]
        }]);
        let current = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [instance(dataset_b)]
        }]);
        let fragment = json!([{
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_a}]
            }]
        }]);
        let addressed = AddressedInstances::EntityFragment {
            fragment: &fragment,
            overwrite: OverwritePolicy::Overwrite,
        };

        let history = mutation_history(addressed, &previous, &current).expect("readable datasetIds");

        // The deletion is recorded by its tombstone alone: the NGSI-LD Null is
        // an instruction, and the instance it removed is not in current state.
        pretty_assertions::assert_eq!(history.len(), 1);
        let HistoryRecord::Tombstone(removed) = &history[0] else {
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
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}]
            })
        };
        let previous = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/temperature": [temperature(25)]}]);
        let current = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/temperature": [temperature(100)]}]);
        let fragment = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 100}]
        });
        let attribute = IriBuf::new("https://example.org/temperature".to_owned()).expect("a valid IRI");
        let addressed = AddressedInstances::AttributeFragment {
            attribute: &attribute,
            dataset_id: None,
            fragment: &fragment,
        };

        let history = mutation_history(addressed, &previous, &current).expect("readable datasetIds");

        pretty_assertions::assert_eq!(history.len(), 1);
        let HistoryRecord::Observations(observed) = &history[0] else {
            panic!("the only record is the observation: {history:?}");
        };
        pretty_assertions::assert_eq!(**observed, current);
    }

    /// A write that applied nothing -- a no-overwrite append of an attribute the
    /// Entity already holds -- records no observation at all.
    #[test]
    fn a_write_that_applied_no_instance_records_no_observation() {
        let state = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]
        }]);
        let fragment = json!([{
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}]
        }]);
        let addressed = AddressedInstances::EntityFragment {
            fragment: &fragment,
            overwrite: OverwritePolicy::Preserve,
        };

        let history = mutation_history(addressed, &state, &state).expect("readable datasetIds");

        assert!(history.is_empty(), "{history:?}");
    }

    /// Clause 4.18: a replacement that omits `scope` ends the Scope's interval.
    #[test]
    fn a_replacement_without_scope_records_the_scope_removal() {
        let previous = json!([{"@id": "urn:ngsi-ld:A", "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}]}]);
        let replacement: Value = json!([{"@id": "urn:ngsi-ld:A"}]);
        let addressed = AddressedInstances::EntityFragment {
            fragment: &replacement,
            overwrite: OverwritePolicy::Overwrite,
        };

        let history = mutation_history(addressed, &previous, &replacement).expect("readable datasetIds");

        assert!(matches!(history.as_slice(), [HistoryRecord::ScopeRemoval]), "{history:?}");
    }

    #[test]
    fn deleting_an_entity_tombstones_every_instance_it_still_held_in_a_fixed_order() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:b"}]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]}
            ],
            "https://example.org/colour": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]
        }]);

        let named = deletion_history(&previous)
            .expect("readable datasetIds")
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
        assert!(deletion_history(&json!([{"@id": "urn:ngsi-ld:A"}])).expect("readable datasetIds").is_empty());
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

    /// Clauses 4.5.7, 4.5.8 and 4.18: a deletion that never read the payload
    /// states the instances and the Scope it removed itself, and records
    /// exactly the tombstones -- each instance with its family, then the Scope
    /// -- that reading the removed payload records.
    #[test]
    fn an_outline_of_every_instance_and_the_scope_records_the_tombstones_the_payload_would() {
        let removed = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}],
            "https://example.org/speed": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:b"}]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]}
            ],
            "https://example.org/owner": [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:P"}]}]
        }]);
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
        pretty_assertions::assert_eq!(ended(deletion_history(&removed).expect("readable datasetIds")), expected);
    }

    #[test]
    fn an_entity_that_held_no_scope_records_no_scope_removal() {
        let outline = vec![instance("https://example.org/speed", None, AttributeType::Property)];
        assert!(!ended(deletion_tombstones(outline, ScopePresence::Absent)).contains(&Ended::Scope));
    }
}
