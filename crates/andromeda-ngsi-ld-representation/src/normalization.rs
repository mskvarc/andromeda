//! Concise input, rewritten as the normalized representation.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.2.3 lets a request write an attribute
//! concisely — a bare value where a `{"type": "Property", "value": …}`
//! envelope would go, and the same abbreviation for any sub-attribute. Clause
//! 4.5.1 defines the normalized form the broker stores and reasons about. This
//! module is the rewrite between them, and it is the inverse of the simplified
//! rendering that answers a query.
//!
//! It is deliberately conservative: an attribute that already declares one of
//! the clause 5.2.4 types is left alone except for its own sub-attributes, and
//! the reserved members of that type are never mistaken for sub-attributes.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::{Map, Value};
use std::str::FromStr;

/// The entity members clause 5.2.4 defines, which are never attributes.
const ENTITY_MEMBERS: &[&str] = &[
    "id",
    "type",
    "scope",
    "@context",
    "@id",
    "@type",
    "expiresAt",
    "createdAt",
    "modifiedAt",
    "deletedAt",
];

/// Rewrite every attribute of an entity into its normalized form.
///
/// A payload that is not an object is left untouched: deciding what it is
/// belongs to whoever validates it, not here.
pub fn normalize_entity_attributes(payload: &mut Value) {
    let Some(entity) = payload.as_object_mut() else {
        return;
    };
    for (member, value) in entity.iter_mut() {
        if ENTITY_MEMBERS.contains(&member.as_str()) {
            continue;
        }
        normalize_attribute(value);
    }
}

/// Rewrite one attribute slot: one instance, several, or a concise value.
fn normalize_attribute(value: &mut Value) {
    match value {
        Value::Object(instance) if declares_an_attribute_type(instance) => normalize_instance(instance),
        Value::Array(instances) if all_declare_an_attribute_type(instances) => {
            for instance in instances {
                if let Value::Object(instance) = instance {
                    normalize_instance(instance);
                }
            }
        }
        // Anything else is the clause 4.5.2.3 concise form: the value of an
        // implicit Property.
        ref _concise => {
            let concise = std::mem::take(value);
            *value = as_property(concise);
        }
    }
}

/// Whether an array holds nothing but attribute instances.
///
/// An empty array is not one: it carries no type to normalize against, and
/// treating it as a concise value is what preserves it verbatim.
fn all_declare_an_attribute_type(instances: &[Value]) -> bool {
    !instances.is_empty() && instances.iter().all(|instance| instance.as_object().is_some_and(declares_an_attribute_type))
}

/// Whether an object declares one of the clause 5.2.4 attribute types.
fn declares_an_attribute_type(instance: &Map<String, Value>) -> bool {
    attribute_type_of(instance).is_some()
}

/// The attribute type an instance declares, if it declares one.
fn attribute_type_of(instance: &Map<String, Value>) -> Option<AttributeType> {
    AttributeType::from_str(instance.get("type")?.as_str()?).ok()
}

/// Rewrite the sub-attributes of one instance, leaving its reserved members
/// alone.
fn normalize_instance(instance: &mut Map<String, Value>) {
    let Some(declared) = attribute_type_of(instance) else {
        return;
    };
    let reserved = declared.reserved_fields();
    for (member, value) in instance.iter_mut() {
        if member == "type" || reserved.contains(&member.as_str()) {
            continue;
        }
        normalize_attribute(value);
    }
}

/// Wrap a concise value as the Property clause 4.5.2.3 says it abbreviates.
fn as_property(value: Value) -> Value {
    let mut property = Map::with_capacity(2);
    property.insert("type".to_string(), Value::String(AttributeType::Property.to_string()));
    property.insert("value".to_string(), value);
    Value::Object(property)
}

#[cfg(test)]
mod tests {
    use crate::normalization::normalize_entity_attributes;
    use serde_json::json;

    #[test]
    fn a_concise_scalar_becomes_a_property_with_that_value() {
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "speed": 42 });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"], json!({ "type": "Property", "value": 42 }));
    }

    #[test]
    fn every_entity_member_is_left_alone() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "scope": "/a/b",
            "expiresAt": "2030-01-01T00:00:00Z",
            "createdAt": "2020-01-01T00:00:00Z",
            "modifiedAt": "2020-01-02T00:00:00Z",
            "deletedAt": "2020-01-03T00:00:00Z"
        });
        let untouched = payload.clone(); // clone: the expectation compares against the input

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload, untouched);
    }

    #[test]
    fn an_attribute_that_already_declares_its_type_keeps_its_envelope() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": { "type": "Property", "value": 42, "observedAt": "2024-01-01T00:00:00Z" }
        });
        let untouched = payload.clone(); // clone: the expectation compares against the input

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload, untouched);
    }

    #[test]
    fn a_concise_sub_attribute_is_wrapped_but_a_reserved_member_is_not() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": { "type": "Property", "value": 42, "unitCode": "KMH", "accuracy": 0.5 }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"]["unitCode"], json!("KMH"));
        pretty_assertions::assert_eq!(payload["speed"]["accuracy"], json!({ "type": "Property", "value": 0.5 }));
        pretty_assertions::assert_eq!(payload["speed"]["value"], json!(42));
    }

    #[test]
    fn every_instance_of_a_multi_dataset_attribute_is_normalized() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": [
                { "type": "Property", "value": 42, "accuracy": 0.5 },
                { "type": "Property", "value": 43, "accuracy": 0.6 }
            ]
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"][0]["accuracy"], json!({ "type": "Property", "value": 0.5 }));
        pretty_assertions::assert_eq!(payload["speed"][1]["accuracy"], json!({ "type": "Property", "value": 0.6 }));
    }

    #[test]
    fn an_array_that_is_not_attribute_instances_is_the_value_of_one_property() {
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "tags": ["a", "b"] });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["tags"], json!({ "type": "Property", "value": ["a", "b"] }));
    }

    #[test]
    fn an_empty_array_is_a_value_rather_than_an_empty_list_of_instances() {
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "tags": [] });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["tags"], json!({ "type": "Property", "value": [] }));
    }

    #[test]
    fn a_payload_that_is_not_an_object_is_left_untouched() {
        let mut payload = json!(["not an entity"]);
        let untouched = payload.clone(); // clone: the expectation compares against the input

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload, untouched);
    }
}
