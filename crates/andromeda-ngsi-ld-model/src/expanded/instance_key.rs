//! Identity of a single attribute instance inside an expanded entity payload.

use crate::expanded::{
    dataset_id::{InstanceDatasetIdError, get_instance_dataset_id, position_of_dataset},
    node::expanded_object_ref,
    reserved_member::is_reserved_member,
};
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
/// Skips JSON-LD keywords and every member NGSI-LD reserves for itself
/// ([`is_reserved_member`]): the system attributes of clause 4.8, the two
/// Entity members that are not attributes (`scope`, clause 4.18, and the
/// Entity's own `expiresAt`, clause 4.22) and the proof seals of clause
/// 4.5.2.2. An Entity-level `expiresAt` read as an instance would be diffed,
/// tombstoned and counted as an attribute named `expiresAt` that the Entity
/// never had. Attribute names that are not valid IRIs are skipped too: they
/// cannot correspond to a temporal catalogue row, whose `attribute_name` is
/// IRI-validated on read.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when an instance carries a `datasetId`
/// that is not an identifier, since its identity cannot be told.
pub fn attribute_instance_families(expanded: &Value) -> Result<HashMap<AttributeInstanceKey, AttributeType>, InstanceDatasetIdError> {
    let mut instances_by_key = HashMap::new();
    let Some(obj) = expanded_object_ref(expanded) else {
        return Ok(instances_by_key);
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
        if is_reserved_member(&attr) {
            continue;
        }
        for instance in instances {
            let dataset_id = get_instance_dataset_id(instance)?;
            instances_by_key.insert(
                AttributeInstanceKey {
                    attr: attr.clone(), // clone: one key per instance, each owning its IRI
                    dataset_id,
                },
                declared_attribute_type(instance),
            );
        }
    }

    Ok(instances_by_key)
}

/// The family of every attribute instance one named member of an expanded
/// payload holds, in order; none when NGSI-LD reserves the member, which holds
/// no attribute instance.
///
/// `name` is a member name already read as an attribute name (as
/// `extract_attribute_names` reads the payload's), so a caller that has it
/// parses no name twice. The members skipped and the family read are those of
/// [`attribute_instance_families`]. The instances are not keyed by
/// `datasetId`: a payload the write path admitted holds each instance of a name
/// once (clause 4.5.5), and a caller counting instances needs every one.
pub fn member_instance_families<'a>(expanded: &'a Value, name: &AttributeName) -> impl Iterator<Item = AttributeType> + use<'a> {
    let instances = if is_reserved_member(name) {
        None
    } else {
        expanded_object_ref(expanded).and_then(|node| node.get(name.as_str())).and_then(Value::as_array)
    };
    instances.into_iter().flatten().map(declared_attribute_type)
}

/// The family one named attribute instance declares in an expanded payload.
///
/// Answers `None` when the payload holds no such instance, which is the case a
/// deletion computed from a catalogue rather than from the payload can reach.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when an instance consulted before the
/// match carries a `datasetId` that is not an identifier.
pub fn attribute_instance_family(expanded: &Value, attr: &AttributeName, dataset_id: Option<&Uri>) -> Result<Option<AttributeType>, InstanceDatasetIdError> {
    let Some(instances) = expanded_object_ref(expanded).and_then(|obj| obj.get(attr.as_str())).and_then(Value::as_array) else {
        return Ok(None);
    };
    Ok(position_of_dataset(instances, dataset_id)?
        .and_then(|position| instances.get(position))
        .map(declared_attribute_type))
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
    use crate::expanded::instance_key::{attribute_instance_families, member_instance_families};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
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
        let indexed = attribute_instance_families(&payload).expect("readable datasetIds");
        pretty_assertions::assert_eq!(indexed.len(), 1);
        assert!(indexed.keys().all(|key| key.attr.as_str() == "https://example.com/a"));
    }

    /// Clause 4.22 puts an `expiresAt` on the Entity itself and clause 4.5.2.2
    /// seals its identifier and types; neither is an attribute instance, so a
    /// replacement or a deletion never tombstones one and discovery never
    /// counts one.
    #[test]
    fn the_entity_expiry_and_the_proof_seals_are_not_attribute_instances() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2030-01-01T00:00:00Z"}],
            "https://uri.etsi.org/ngsi-ld/entityIdSealed": [{"@id": "urn:ngsi-ld:Vehicle:1"}],
            "https://uri.etsi.org/ngsi-ld/entityTypeSealed": [{"@id": "https://example.com/Vehicle"}],
            "https://example.com/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}],
        }]);
        let indexed = attribute_instance_families(&payload).expect("readable datasetIds");
        let names: Vec<&str> = indexed.keys().map(|key| key.attr.as_str()).collect();
        pretty_assertions::assert_eq!(names, vec!["https://example.com/a"]);
    }

    /// The shapes walk reads the same members and families as the keyed walk,
    /// one entry per instance, from names the caller already parsed.
    #[test]
    fn the_instance_shapes_agree_with_the_keyed_instance_families() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/A"}],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": "2030-01-01T00:00:00Z"}],
            "https://example.com/rel": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]}
            ],
            "https://example.com/plain": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}],
        }]);
        let names: Vec<AttributeName> = payload[0]
            .as_object()
            .expect("a node")
            .keys()
            .filter(|key| !key.starts_with('@'))
            .map(|key| key.parse().expect("a valid attribute name"))
            .collect();
        let mut shapes: Vec<(String, AttributeType)> = names
            .iter()
            .flat_map(|name| member_instance_families(&payload, name).map(move |family| (name.as_str().to_owned(), family)))
            .collect();
        shapes.sort_by(|left, right| (left.0.as_str(), left.1.to_string()).cmp(&(right.0.as_str(), right.1.to_string())));
        let mut keyed: Vec<(String, AttributeType)> = attribute_instance_families(&payload)
            .expect("readable datasetIds")
            .into_iter()
            .map(|(key, family)| (key.attr.as_str().to_owned(), family))
            .collect();
        keyed.sort_by(|left, right| (left.0.as_str(), left.1.to_string()).cmp(&(right.0.as_str(), right.1.to_string())));
        pretty_assertions::assert_eq!(shapes, keyed);
        pretty_assertions::assert_eq!(shapes.len(), 3);
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
        let indexed = attribute_instance_families(&payload).expect("readable datasetIds");
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
        let indexed = attribute_instance_families(&payload).expect("readable datasetIds");
        pretty_assertions::assert_eq!(indexed.values().copied().collect::<Vec<AttributeType>>(), vec![AttributeType::Property]);
    }
}
