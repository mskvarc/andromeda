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

/// Build the observations and tombstones produced by a full entity
/// replacement under NGSI-LD clause 5.6.3.
pub fn replacement_history(observed: &Arc<Value>, previous: &Value, replacement: &Value) -> Vec<HistoryRecord> {
    let removed = removed_attribute_instances(previous, replacement);
    let mut history = Vec::with_capacity(removed.len() + 1);
    history.push(HistoryRecord::Observations(Arc::clone(observed))); // clone: history and change-event paths share the observed payload.
    history.extend(removed.into_iter().map(HistoryRecord::Tombstone));
    history
}

#[cfg(test)]
mod tests {
    use crate::history_record::{HistoryRecord, replacement_history};
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
        let history = replacement_history(&observed, &previous, &replacement);
        assert!(history[0].is_observations());
        assert!(history.iter().any(HistoryRecord::is_tombstone));
    }
}
