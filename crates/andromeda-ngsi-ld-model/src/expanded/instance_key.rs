//! Identity of a single attribute instance inside an expanded entity payload.

use crate::expanded::{dataset_id::get_instance_dataset_id, node::expanded_object_ref};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;
use std::collections::HashMap;

/// Returns whether the expanded IRI names an NGSI-LD system attribute.
///
/// System attributes are broker-managed metadata (`createdAt`, `observedAt`,
/// …) rather than user attributes, so they never take part in the temporal
/// catalogue or in attribute-level diffing.
#[must_use]
pub fn is_system_attribute(attr_name: &AttributeName) -> bool {
    attr_name.as_str() == properties::CREATED_AT.as_str()
        || attr_name.as_str() == properties::DATASET_ID.as_str()
        || attr_name.as_str() == properties::DELETED_AT.as_str()
        || attr_name.as_str() == properties::INSTANCE_ID.as_str()
        || attr_name.as_str() == properties::MODIFIED_AT.as_str()
        || attr_name.as_str() == properties::OBSERVED_AT.as_str()
        || attr_name.as_str() == properties::UNIT_CODE.as_str()
}

/// Identity of one attribute instance inside an entity payload: the expanded
/// attribute IRI together with its multi-attribute discriminator
/// (NGSI-LD 5.2.5).
///
/// This is the grain at which each attribute instance is tracked, which is what
/// makes it the right key for diffing a replacement payload against the one it
/// supersedes: every key that disappears needs a deletion recorded so an
/// entity's temporal history and its current-value view stay in step with the
/// stored entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttributeInstanceKey {
    /// Expanded attribute IRI.
    pub attr: AttributeName,
    /// `datasetId`, or `None` for the default instance.
    pub dataset_id: Option<Uri>,
}

/// Index every attribute instance of an expanded entity payload by its identity,
/// against the attribute family that instance declares.
///
/// The family is carried beside the identity because a deletion has to record
/// it: clauses 4.5.7 and 4.5.8 render an instance that has ended with the
/// `urn:ngsi-ld:null` member its family prescribes — `object` for a
/// Relationship, a `languageMap` entry for a `LanguageProperty`, and so on — and
/// once the instance is gone from current state there is nowhere else left to
/// read it from. An instance that declares no family is a Property (clause
/// 4.5.2).
///
/// Skips JSON-LD keywords, NGSI-LD system attributes, and `scope` (which is not
/// an attribute — clause 4.5.4). Attribute names that are not valid IRIs are
/// skipped too: they cannot correspond to a temporal catalogue row, whose
/// `attribute_name` is IRI-validated on read.
#[must_use]
pub fn attribute_instance_families(expanded: &Value) -> HashMap<AttributeInstanceKey, AttributeType> {
    let mut instances_by_key = HashMap::new();
    let Some(obj) = expanded_object_ref(expanded) else {
        return instances_by_key;
    };

    for (attr_name, attr_value) in obj {
        if attr_name.starts_with('@') {
            continue;
        }
        let Some(instances) = attr_value.as_array() else {
            continue;
        };
        let Ok(attr) = attr_name.parse::<AttributeName>() else {
            continue;
        };
        if is_system_attribute(&attr) || attr.as_str() == properties::SCOPE.as_str() {
            continue;
        }
        for instance in instances {
            let dataset_id = get_instance_dataset_id(instance);
            instances_by_key.insert(
                AttributeInstanceKey {
                    attr: attr.clone(), // clone: one key per instance, each owning its IRI
                    dataset_id,
                },
                declared_attribute_type(instance),
            );
        }
    }

    instances_by_key
}

/// The family one named attribute instance declares in an expanded payload.
///
/// Answers `None` when the payload holds no such instance, which is the case a
/// deletion computed from a catalogue rather than from the payload can reach.
pub fn attribute_instance_family(expanded: &Value, attr: &AttributeName, dataset_id: Option<&Uri>) -> Option<AttributeType> {
    expanded_object_ref(expanded)?
        .get(attr.as_str())?
        .as_array()?
        .iter()
        .find(|instance| get_instance_dataset_id(instance).as_ref() == dataset_id)
        .map(declared_attribute_type)
}

/// The attribute family one expanded instance declares.
///
/// Clause 4.5.2 makes `Property` the family of an attribute that declares none,
/// which is also what an instance carrying an unrecognised `@type` amounts to.
pub(crate) fn declared_attribute_type(instance: &Value) -> AttributeType {
    instance
        .get("@type")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(AttributeType::from_expanded_iri)
        .unwrap_or(AttributeType::Property)
}

#[cfg(test)]
mod tests {
    use crate::expanded::instance_key::attribute_instance_families;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::json;

    #[test]
    fn attribute_instance_families_skips_keywords_system_attributes_and_scope() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/createdAt": [{"@value": "2024-01-01T00:00:00Z"}],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/A"}],
            "https://example.com/a": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]
            }],
        }]);
        let indexed = attribute_instance_families(&payload);
        pretty_assertions::assert_eq!(indexed.len(), 1);
        assert!(indexed.keys().all(|key| key.attr.as_str() == "https://example.com/a"));
    }

    /// Clauses 4.5.7 and 4.5.8 render an ended instance with the null member of
    /// its own family, so every family an instance can declare is indexed.
    #[test]
    fn each_declared_family_is_carried_beside_the_instance_identity() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/rel": [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"]}],
            "https://example.com/label": [{"@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"]}],
            "https://example.com/spot": [{"@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"]}],
            "https://example.com/vocab": [{"@type": ["https://uri.etsi.org/ngsi-ld/VocabProperty"]}],
            "https://example.com/doc": [{"@type": ["https://uri.etsi.org/ngsi-ld/JsonProperty"]}],
        }]);
        let indexed = attribute_instance_families(&payload);
        let family = |name: &str| {
            indexed
                .iter()
                .find(|(key, _)| key.attr.as_str() == name)
                .map(|(_, attribute_type)| *attribute_type)
        };
        pretty_assertions::assert_eq!(family("https://example.com/rel"), Some(AttributeType::Relationship));
        pretty_assertions::assert_eq!(family("https://example.com/label"), Some(AttributeType::LanguageProperty));
        pretty_assertions::assert_eq!(family("https://example.com/spot"), Some(AttributeType::GeoProperty));
        pretty_assertions::assert_eq!(family("https://example.com/vocab"), Some(AttributeType::VocabProperty));
        pretty_assertions::assert_eq!(family("https://example.com/doc"), Some(AttributeType::JsonProperty));
    }

    /// Clause 4.5.2: an instance that declares no family is a Property.
    #[test]
    fn an_instance_declaring_no_family_is_indexed_as_a_property() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/a": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}],
        }]);
        let indexed = attribute_instance_families(&payload);
        pretty_assertions::assert_eq!(indexed.values().copied().collect::<Vec<AttributeType>>(), vec![AttributeType::Property]);
    }
}
