//! Temporal Evolution consequences carried by an atomic write.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::instance_key::{AttributeInstanceKey, removed_attribute_instances};
use derive_more::IsVariant;
use serde_json::Value;
use std::sync::Arc;

/// One temporal-history consequence of an entity mutation.
#[derive(Debug, IsVariant)]
pub enum HistoryRecord {
    /// Attribute observations contained in an expanded payload.
    Observations(Arc<Value>),
    /// End of one attribute instance that left current entity state.
    Tombstone(AttributeInstanceKey),
}

/// Build a tombstone for one validated attribute-instance identity.
pub fn tombstone(attr: &AttributeName, dataset_id: Option<&Uri>) -> HistoryRecord {
    HistoryRecord::Tombstone(AttributeInstanceKey {
        attr: attr.clone(),              // clone: the history record owns its attribute name.
        dataset_id: dataset_id.cloned(), // clone: the history record owns its dataset identifier.
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

#[cfg(test)]
mod tests {
    use crate::history_record::{HistoryRecord, mutation_history};
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
        let HistoryRecord::Tombstone(key) = &history[1] else {
            return;
        };
        pretty_assertions::assert_eq!(key.attr.as_str(), "https://example.com/speed");
        pretty_assertions::assert_eq!(key.dataset_id.as_ref().map(Uri::as_str), Some(dataset_a));
    }
}
