//! Temporal Evolution consequences carried by an atomic write.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    instance_key::{AttributeInstanceKey, attribute_instance_families},
    removed_instance::{RemovedAttributeInstance, removed_attribute_instances},
};
use derive_more::IsVariant;
use serde_json::Value;
use std::sync::Arc;

/// One temporal-history consequence of an entity mutation.
#[derive(Debug, IsVariant)]
pub enum HistoryRecord {
    /// Attribute observations contained in an expanded payload.
    Observations(Arc<Value>),
    /// End of one attribute instance that left current entity state.
    Tombstone(RemovedAttributeInstance),
}

/// Build a tombstone for one validated attribute-instance identity.
///
/// The family travels with the record because a deletion renders the
/// `urn:ngsi-ld:null` member of the family the instance belonged to (clauses
/// 4.5.7 and 4.5.8), and current state no longer holds the instance to ask.
pub fn tombstone(attr: &AttributeName, dataset_id: Option<&Uri>, attribute_type: AttributeType) -> HistoryRecord {
    HistoryRecord::Tombstone(RemovedAttributeInstance {
        key: AttributeInstanceKey {
            attr: attr.clone(),              // clone: the history record owns its attribute name.
            dataset_id: dataset_id.cloned(), // clone: the history record owns its dataset identifier.
        },
        attribute_type,
    })
}

/// Build the observations and exact instance tombstones produced by a payload mutation.
pub fn mutation_history(observed: &Arc<Value>, previous: &Value, current: &Value) -> Vec<HistoryRecord> {
    let removed = removed_attribute_instances(previous, current);
    let mut history = Vec::with_capacity(removed.len() + 1);
    history.push(HistoryRecord::Observations(Arc::clone(observed))); // clone: history and change-event paths share the observed payload.
    history.extend(removed.into_iter().map(HistoryRecord::Tombstone));
    history
}

/// Build the tombstone every attribute instance of a deleted entity needs.
///
/// Clause 5.6.6 deletes the entity's current state while leaving its Temporal
/// Evolution standing, and clause 4.5.8 has an instance that has ended carry a
/// `deletedAt`. The deletion therefore records one tombstone per instance the
/// entity still held, so temporal history says when each instance ended instead
/// of trailing off with no explanation.
///
/// The order is fixed by the instance identity rather than left to the set that
/// collected it: a tombstone's position in the write is half of the key its
/// stored row deduplicates on, so a replay has to produce the same order.
pub fn deletion_history(previous: &Value) -> Vec<HistoryRecord> {
    let mut removed = attribute_instance_families(previous)
        .into_iter()
        .map(|(key, attribute_type)| RemovedAttributeInstance { key, attribute_type })
        .collect::<Vec<RemovedAttributeInstance>>();
    removed.sort_by(|left, right| {
        left.key.attr.as_str().cmp(right.key.attr.as_str()).then_with(|| {
            left.key
                .dataset_id
                .as_ref()
                .map(Uri::as_str)
                .cmp(&right.key.dataset_id.as_ref().map(Uri::as_str))
        })
    });
    removed.into_iter().map(HistoryRecord::Tombstone).collect()
}

#[cfg(test)]
mod tests {
    use crate::history_record::{HistoryRecord, deletion_history, mutation_history};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::sync::Arc;

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
        let observed = Arc::new(replacement.clone()); // clone: the test compares the replacement after sharing it.
        let history = mutation_history(&observed, &previous, &replacement);
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
        let observed = Arc::new(json!([{
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_a}]
            }]
        }]));

        let history = mutation_history(&observed, &previous, &current);

        pretty_assertions::assert_eq!(history.len(), 2);
        assert!(history[1].is_tombstone());
        let HistoryRecord::Tombstone(removed) = &history[1] else {
            return;
        };
        pretty_assertions::assert_eq!(removed.key.attr.as_str(), "https://example.com/speed");
        pretty_assertions::assert_eq!(removed.key.dataset_id.as_ref().map(Uri::as_str), Some(dataset_a));
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
            .into_iter()
            .map(|entry| match entry {
                HistoryRecord::Tombstone(removed) => (removed.key.attr.as_str().to_owned(), removed.key.dataset_id.map(|id| id.as_str().to_owned())),
                HistoryRecord::Observations(_) => (String::new(), None),
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
        assert!(deletion_history(&json!([{"@id": "urn:ngsi-ld:A"}])).is_empty());
    }
}
