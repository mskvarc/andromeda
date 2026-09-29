//! Attribute instances a write drops from an entity, with the family each held.

use crate::expanded::{
    dataset_id::InstanceDatasetIdError,
    instance_key::{AttributeInstanceKey, attribute_instance_families},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::Value;

/// One attribute instance that has left an entity's current state.
///
/// The family is part of the record rather than something a later reader
/// re-derives: clauses 4.5.7 and 4.5.8 render an ended instance with the
/// `urn:ngsi-ld:null` member its family prescribes, and by the time the deletion
/// is stored the instance it describes is no longer in the payload to ask.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RemovedAttributeInstance {
    /// Identity of the instance that was removed.
    pub key: AttributeInstanceKey,
    /// Attribute family the removed instance declared.
    pub attribute_type: AttributeType,
}

/// Attribute instances present in `previous` but absent from `replacement`.
///
/// These are the instances a full replacement (`PUT`, replace-mode upsert,
/// attribute purge) silently drops from the stored entity. Each one needs a
/// tombstone so the entity's temporal history records the deletion and its
/// current-value view stops reporting a value that no longer exists.
///
/// Identity is the attribute name and `datasetId` pair alone (clause 5.2.5), so
/// an instance whose family changed is an update rather than a removal; the
/// family reported here is the one the removed instance held.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when either payload carries an instance
/// whose `datasetId` is not an identifier.
pub fn removed_attribute_instances(previous: &Value, replacement: &Value) -> Result<Vec<RemovedAttributeInstance>, InstanceDatasetIdError> {
    let retained = attribute_instance_families(replacement)?;
    Ok(attribute_instance_families(previous)?
        .into_iter()
        .filter(|(key, _)| !retained.contains_key(key))
        .map(|(key, attribute_type)| RemovedAttributeInstance { key, attribute_type })
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::expanded::removed_instance::{RemovedAttributeInstance, removed_attribute_instances};
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
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
        let removed = removed_attribute_instances(&previous, &replacement).expect("readable datasetIds");
        pretty_assertions::assert_eq!(removed.len(), 1);
        pretty_assertions::assert_eq!(removed[0].key.attr.as_str(), "https://example.com/b");
        assert!(removed[0].key.dataset_id.is_none());
    }

    #[test]
    fn removed_instances_is_empty_when_nothing_was_dropped() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/a": [property(1)],
        }]);
        pretty_assertions::assert_eq!(removed_attribute_instances(&payload, &payload), Ok(Vec::<RemovedAttributeInstance>::new()));
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
        let removed = removed_attribute_instances(&previous, &replacement).expect("readable datasetIds");
        pretty_assertions::assert_eq!(removed.len(), 1);
        pretty_assertions::assert_eq!(removed[0].key.dataset_id.as_ref().map(Identifier::as_str), Some("urn:ngsi-ld:Dataset:b"));
    }

    #[test]
    fn removed_instances_accepts_non_urn_dataset_ids() {
        // NGSI-LD 5.2.5 types datasetId as a URI, not a URN.
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [property_in_dataset(1, "http://example.org/d1")],
        }]);
        let replacement = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}]);
        let removed = removed_attribute_instances(&previous, &replacement).expect("readable datasetIds");
        pretty_assertions::assert_eq!(removed.len(), 1);
        pretty_assertions::assert_eq!(removed[0].key.dataset_id.as_ref().map(Identifier::as_str), Some("http://example.org/d1"));
    }

    /// A Relationship that leaves the entity is recorded as a Relationship, so
    /// its deletion can render `"object": "urn:ngsi-ld:null"` (clause 4.5.8)
    /// rather than a Property value it never had.
    #[test]
    fn a_removed_instance_carries_the_family_it_declared() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/rel": [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"]}],
            "https://example.com/label": [{"@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"]}],
        }]);
        let replacement = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}]);
        let removed = removed_attribute_instances(&previous, &replacement).expect("readable datasetIds");
        let family = |name: &str| {
            removed
                .iter()
                .find(|instance| instance.key.attr.as_str() == name)
                .map(|instance| instance.attribute_type)
        };
        pretty_assertions::assert_eq!(family("https://example.com/rel"), Some(AttributeType::Relationship));
        pretty_assertions::assert_eq!(family("https://example.com/label"), Some(AttributeType::LanguageProperty));
    }
}
