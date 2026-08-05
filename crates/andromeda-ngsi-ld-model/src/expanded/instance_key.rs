//! Identity of a single attribute instance inside an expanded entity payload.

use crate::expanded::{dataset_id::get_instance_dataset_id, node::expanded_object_ref};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use iri_rs::IriBuf;
use serde_json::Value;
use std::collections::HashSet;

/// Returns whether the expanded IRI names an NGSI-LD system attribute.
///
/// System attributes are broker-managed metadata (`createdAt`, `observedAt`,
/// …) rather than user attributes, so they never take part in the temporal
/// catalogue or in attribute-level diffing.
pub fn is_system_attribute(attr_name: &str) -> bool {
    attr_name == properties::CREATED_AT.as_str()
        || attr_name == properties::DATASET_ID.as_str()
        || attr_name == properties::DELETED_AT.as_str()
        || attr_name == properties::INSTANCE_ID.as_str()
        || attr_name == properties::MODIFIED_AT.as_str()
        || attr_name == properties::OBSERVED_AT.as_str()
        || attr_name == properties::UNIT_CODE.as_str()
}

/// Identity of one attribute instance inside an entity payload: the expanded
/// attribute IRI together with its multi-attribute discriminator
/// (NGSI-LD 5.2.5).
///
/// This is the same grain as a `temporal_entity_attributes` row, which is what
/// makes it the right key for diffing a replacement payload against the one it
/// supersedes: every key that disappears needs a deletion recorded so the
/// temporal history and the `attribute_current` projection stay in step with
/// `entities.payload`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttributeInstanceKey {
    /// Expanded attribute IRI.
    pub attr: IriBuf,
    /// `datasetId`, or `None` for the default instance.
    pub dataset_id: Option<IriBuf>,
}

/// Collect every attribute instance key present in an expanded entity payload.
///
/// Skips JSON-LD keywords, NGSI-LD system attributes, and `scope` (which is not
/// an attribute — clause 4.5.4). Attribute names that are not valid IRIs are
/// skipped too: they cannot correspond to a temporal catalogue row, whose
/// `attribute_name` is IRI-validated on read.
pub fn attribute_instance_keys(expanded: &Value) -> HashSet<AttributeInstanceKey> {
    let mut keys = HashSet::new();
    let Some(obj) = expanded_object_ref(expanded) else {
        return keys;
    };

    for (attr_name, attr_value) in obj {
        if attr_name.starts_with('@') || is_system_attribute(attr_name) || attr_name == properties::SCOPE.as_str() {
            continue;
        }
        let Some(instances) = attr_value.as_array() else {
            continue;
        };
        // clone: `IriBuf::new` takes ownership of the buffer it validates, and the
        // map key behind `attr_name` is borrowed from the caller's payload.
        let Ok(attr) = IriBuf::new(attr_name.clone()) else {
            continue;
        };
        for instance in instances {
            let dataset_id = get_instance_dataset_id(instance).and_then(|raw| IriBuf::new(raw).ok());
            keys.insert(AttributeInstanceKey {
                attr: attr.clone(), // clone: one key per instance, each owning its IRI
                dataset_id,
            });
        }
    }

    keys
}

/// Attribute instances present in `previous` but absent from `replacement`.
///
/// These are the instances a full replacement (`PUT`, replace-mode upsert,
/// attribute purge) silently drops from `entities.payload`. Each one needs a
/// tombstone so the temporal history records the deletion and the
/// `attribute_current` projection stops reporting a value that no longer
/// exists.
pub fn removed_attribute_instances(previous: &Value, replacement: &Value) -> Vec<AttributeInstanceKey> {
    let retained = attribute_instance_keys(replacement);
    attribute_instance_keys(previous).into_iter().filter(|key| !retained.contains(key)).collect()
}

#[cfg(test)]
mod tests {
    use crate::expanded::instance_key::{attribute_instance_keys, removed_attribute_instances};
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    fn property(value: i64) -> Value {
        json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        })
    }

    fn property_in_dataset(value: i64, dataset_id: &str) -> Value {
        json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
        })
    }

    #[test]
    fn removed_instances_reports_attributes_a_replacement_drops() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/a": [property(1)],
            "https://example.com/b": [property(2)],
        }]);
        let replacement = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/a": [property(1)],
        }]);
        let removed = removed_attribute_instances(&previous, &replacement);
        pretty_assertions::assert_eq!(removed.len(), 1);
        pretty_assertions::assert_eq!(removed[0].attr.as_str(), "https://example.com/b");
        assert!(removed[0].dataset_id.is_none());
    }

    #[test]
    fn removed_instances_is_empty_when_nothing_was_dropped() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/a": [property(1)],
        }]);
        assert!(removed_attribute_instances(&payload, &payload).is_empty());
    }

    #[test]
    fn removed_instances_distinguishes_dataset_partitioned_instances() {
        // Dropping one datasetId of a multi-instance attribute is a deletion of
        // that instance alone; the surviving instance must not be tombstoned.
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [
                property_in_dataset(1, "urn:ngsi-ld:Dataset:a"),
                property_in_dataset(2, "urn:ngsi-ld:Dataset:b"),
            ],
        }]);
        let replacement = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [property_in_dataset(1, "urn:ngsi-ld:Dataset:a")],
        }]);
        let removed = removed_attribute_instances(&previous, &replacement);
        pretty_assertions::assert_eq!(removed.len(), 1);
        pretty_assertions::assert_eq!(removed[0].dataset_id.as_ref().map(IriBuf::as_str), Some("urn:ngsi-ld:Dataset:b"));
    }

    #[test]
    fn removed_instances_accepts_non_urn_dataset_ids() {
        // NGSI-LD 5.2.5 types datasetId as a URI, not a URN.
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [property_in_dataset(1, "http://example.org/d1")],
        }]);
        let replacement = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}]);
        let removed = removed_attribute_instances(&previous, &replacement);
        pretty_assertions::assert_eq!(removed.len(), 1);
        pretty_assertions::assert_eq!(removed[0].dataset_id.as_ref().map(IriBuf::as_str), Some("http://example.org/d1"));
    }

    #[test]
    fn attribute_instance_keys_skips_keywords_system_attributes_and_scope() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/createdAt": [{"@value": "2024-01-01T00:00:00Z"}],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/A"}],
            "https://example.com/a": [property(1)],
        }]);
        let keys = attribute_instance_keys(&payload);
        pretty_assertions::assert_eq!(keys.len(), 1);
        assert!(keys.iter().all(|key| key.attr.as_str() == "https://example.com/a"));
    }
}
