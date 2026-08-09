//! What an atomic write adds to an entity's Temporal Evolution.
//!
//! The policy states what happened — these observations were made, this
//! attribute instance went away — rather than handing over rows. A history row
//! needs a catalogue identifier that only exists inside the writing
//! transaction, so rows are the store's vocabulary and events are this
//! module's.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::instance_key::{AttributeInstanceKey, removed_attribute_instances};
use derive_more::IsVariant;
use serde_json::Value;
use std::sync::Arc;

/// One thing an atomic write records about an entity's attributes.
#[derive(Debug, IsVariant)]
pub enum HistoryRecord {
    /// Every attribute observation carried by this expanded payload.
    ///
    /// The payload is whatever was observed, which is not always the entity's
    /// new payload: a clause 5.6.1 merge records the patch it applied, not the
    /// entity the patch produced.
    Observations(Arc<Value>),
    /// A tombstone for one attribute instance that is no longer present.
    ///
    /// Written whenever an instance leaves `entities.payload` — an explicit
    /// deletion (clause 5.6.5), an attribute the replacement omitted (clause
    /// 5.6.3), or an instance whose `expiresAt` passed (clause 4.22). Without
    /// it the Temporal Evolution never shows the attribute ending and the
    /// `attribute_current` projection keeps answering `q` filters with the last
    /// value it saw.
    Tombstone(AttributeInstanceKey),
}

/// A tombstone for one validated attribute instance identity.
pub fn tombstone(attr: &AttributeName, dataset_id: Option<&Uri>) -> HistoryRecord {
    HistoryRecord::Tombstone(AttributeInstanceKey {
        attr: attr.clone(),              // clone: the history record owns the attribute name.
        dataset_id: dataset_id.cloned(), // clone: the history record owns the dataset URI.
    })
}

/// The history a full replacement records: its observations, then a tombstone
/// for every attribute instance the replacement drops.
///
/// A replacement overwrites `entities.payload` wholesale, so an attribute the
/// new payload omits is deleted (NGSI-LD 5.6.3) even though no deletion was
/// requested.
pub fn replacement_history(observed: &Arc<Value>, previous: &Value, replacement: &Value) -> Vec<HistoryRecord> {
    let removed = removed_attribute_instances(previous, replacement);
    let mut history = Vec::with_capacity(removed.len() + 1);
    history.push(HistoryRecord::Observations(Arc::clone(observed))); // clone: Arc ref-count bump; the payload is shared with the change-event path
    history.extend(removed.into_iter().map(HistoryRecord::Tombstone));
    history
}

#[cfg(test)]
mod tests {
    use super::{HistoryRecord, replacement_history};
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn instance(dataset_id: Option<&str>) -> Value {
        match dataset_id {
            Some(id) => json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": id}]
            }),
            None => json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]
            }),
        }
    }

    fn tombstones(history: &[HistoryRecord]) -> Vec<(String, Option<String>)> {
        history
            .iter()
            .filter_map(|record| match record {
                HistoryRecord::Tombstone(key) => Some((key.attr.to_string(), key.dataset_id.as_ref().map(ToString::to_string))),
                HistoryRecord::Observations(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_replacement_tombstones_every_dropped_instance() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [instance(None)],
            "https://example.org/colour": [instance(None)]
        }]);
        let replacement = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [instance(None)]
        }]);
        let observed = Arc::new(replacement.clone()); // clone: the test needs both the shared payload and a borrow to diff against

        let history = replacement_history(&observed, &previous, &replacement);

        assert!(history[0].is_observations());
        pretty_assertions::assert_eq!(tombstones(&history), vec![("https://example.org/colour".to_string(), None)]);
    }

    #[test]
    fn a_replacement_that_drops_one_dataset_tombstones_only_that_one() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [instance(Some("urn:d1")), instance(Some("urn:d2"))]
        }]);
        let replacement = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [instance(Some("urn:d1"))]
        }]);
        let observed = Arc::new(replacement.clone()); // clone: the test needs both the shared payload and a borrow to diff against

        let history = replacement_history(&observed, &previous, &replacement);

        pretty_assertions::assert_eq!(
            tombstones(&history),
            vec![("https://example.org/speed".to_string(), Some("urn:d2".to_string()))]
        );
    }

    #[test]
    fn a_replacement_that_drops_nothing_records_only_observations() {
        let previous = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [instance(None)]}]);
        let replacement = previous.clone(); // clone: the two sides of the diff are compared, not moved
        let observed = Arc::new(replacement.clone()); // clone: the test needs both the shared payload and a borrow to diff against

        let history = replacement_history(&observed, &previous, &replacement);

        pretty_assertions::assert_eq!(history.len(), 1);
        assert!(history[0].is_observations());
    }
}
