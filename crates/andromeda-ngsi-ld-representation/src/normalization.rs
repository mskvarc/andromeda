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
//! Where clause 4.5 allows the type to be omitted, it is inferred rather than
//! assumed: an `object` member makes a Relationship, `languageMap` a
//! LanguageProperty, a bare GeoJSON geometry a GeoProperty, and so on — only a
//! value with no such marker becomes an implicit Property.

use crate::entity_members::ENTITY_MEMBERS;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::{Map, Value};
use std::str::FromStr;

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
    if let Value::Object(instance) = value {
        // A declared type, or one clause 4.5 infers from a distinguishing
        // member (`object`, `languageMap`, ...), is an attribute instance:
        // name the type where it was omitted, then normalize its
        // sub-attributes.
        if declares_an_attribute_type(instance) || distinguishing_member_type(instance).is_some() {
            normalize_concise_instance(instance);
            return;
        }
        // The geospatial exception (clause 4.5.2.3): a bare GeoJSON geometry is
        // the value of a GeoProperty, not of a Property.
        if is_geojson_geometry(instance) {
            let geometry = std::mem::take(value);
            *value = as_typed_value(AttributeType::GeoProperty, geometry);
            return;
        }
        // Any other object carries no type to infer, so it is the value of an
        // implicit Property.
        let concise = std::mem::take(value);
        *value = as_typed_value(AttributeType::Property, concise);
        return;
    }

    if let Value::Array(instances) = value
        && all_are_instances(instances)
    {
        for instance in instances {
            if let Value::Object(instance) = instance {
                normalize_concise_instance(instance);
            }
        }
        return;
    }

    // A bare scalar, or an array that is not entirely instances, is the value
    // of an implicit Property (clause 4.5.2.3).
    let concise = std::mem::take(value);
    *value = as_typed_value(AttributeType::Property, concise);
}

/// Normalize one attribute instance, naming its type first if it was left to
/// be inferred from a distinguishing member (clause 4.5.3.3 and siblings).
fn normalize_concise_instance(instance: &mut Map<String, Value>) {
    if !declares_an_attribute_type(instance)
        && let Some(inferred) = distinguishing_member_type(instance)
    {
        instance.insert("type".to_string(), Value::String(inferred.to_string()));
    }
    normalize_instance(instance);
}

/// Whether an array holds nothing but attribute instances.
///
/// An instance either declares its type or lets one be inferred from a
/// distinguishing member. An empty array is not a list of instances: it
/// carries nothing to normalize against, and treating it as a concise value is
/// what preserves it verbatim.
fn all_are_instances(instances: &[Value]) -> bool {
    !instances.is_empty()
        && instances.iter().all(|instance| {
            instance
                .as_object()
                .is_some_and(|instance| declares_an_attribute_type(instance) || distinguishing_member_type(instance).is_some())
        })
}

/// Whether an object declares one of the clause 5.2.4 attribute types.
fn declares_an_attribute_type(instance: &Map<String, Value>) -> bool {
    attribute_type_of(instance).is_some()
}

/// The attribute type an instance declares, if it declares one.
fn attribute_type_of(instance: &Map<String, Value>) -> Option<AttributeType> {
    AttributeType::from_str(instance.get("type")?.as_str()?).ok()
}

/// The attribute type a typeless concise instance's distinguishing member
/// implies (ETSI GS CIM 009 v1.9.1 clauses 4.5.3.3, 4.5.18.3, 4.5.20.3,
/// 4.5.21.3, 4.5.22.3, 4.5.24.3): `object` a Relationship, `languageMap` a
/// LanguageProperty, `vocab` a VocabProperty, `valueList` a ListProperty,
/// `objectList` a ListRelationship, `json` a JsonProperty.
///
/// The member table is [`AttributeType::compact_value_key`], the same one the
/// rendering inverts, iterated so a ninth attribute type cannot be added to
/// the lexicon and forgotten here. `value` is not a distinguishing member: a
/// bare value is an implicit Property, and a GeoProperty is inferred from its
/// geometry shape. More than one distinguishing member is ambiguous, so no
/// type is inferred and the object falls through to an implicit Property.
fn distinguishing_member_type(instance: &Map<String, Value>) -> Option<AttributeType> {
    let mut inferred = None;
    for candidate in AttributeType::ALL {
        let member = candidate.compact_value_key();
        if member == AttributeType::Property.compact_value_key() {
            continue;
        }
        if instance.contains_key(member) {
            if inferred.is_some() {
                return None;
            }
            inferred = Some(candidate);
        }
    }
    inferred
}

/// The GeoJSON geometry type tokens, per RFC 7946 clause 1.4.
///
/// Kept as a local token set rather than pulling in the geometry crate: this
/// crate is a low-layer leaf, and inference needs only to recognise the shape,
/// not decode it.
const GEOJSON_GEOMETRY_TYPES: &[&str] = &[
    "Point",
    "MultiPoint",
    "LineString",
    "MultiLineString",
    "Polygon",
    "MultiPolygon",
    "GeometryCollection",
];

/// Whether an object is a GeoJSON geometry: a known geometry `type` alongside
/// its coordinate member (`coordinates`, or `geometries` for a
/// `GeometryCollection`).
fn is_geojson_geometry(instance: &Map<String, Value>) -> bool {
    let Some(type_name) = instance.get("type").and_then(Value::as_str) else {
        return false;
    };
    if !GEOJSON_GEOMETRY_TYPES.contains(&type_name) {
        return false;
    }
    let coordinate_member = if type_name == "GeometryCollection" { "geometries" } else { "coordinates" };
    instance.contains_key(coordinate_member)
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

/// Wrap a concise value as the `{type, value}` attribute clause 4.5.2.3 says it
/// abbreviates: an implicit Property, or a GeoProperty for a bare geometry.
fn as_typed_value(attribute_type: AttributeType, value: Value) -> Value {
    let mut attribute = Map::with_capacity(2);
    attribute.insert("type".to_string(), Value::String(attribute_type.to_string()));
    attribute.insert("value".to_string(), value);
    Value::Object(attribute)
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

    #[test]
    fn a_typeless_object_member_is_inferred_a_relationship() {
        // Clause 4.5.3.3: a concise Relationship may omit `type`; it is inferred
        // from the `object` member, not stored as a Property.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "belongsTo": { "object": "urn:ngsi-ld:Site:1" } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["belongsTo"], json!({ "type": "Relationship", "object": "urn:ngsi-ld:Site:1" }));
    }

    #[test]
    fn a_typeless_language_map_is_inferred_a_language_property() {
        // Clause 4.5.18.3.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "label": { "languageMap": { "en": "Door", "de": "Tuer" } } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["label"],
            json!({ "type": "LanguageProperty", "languageMap": { "en": "Door", "de": "Tuer" } })
        );
    }

    #[test]
    fn a_typeless_vocab_is_inferred_a_vocab_property() {
        // Clause 4.5.20.3.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "category": { "vocab": "sensor" } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["category"], json!({ "type": "VocabProperty", "vocab": "sensor" }));
    }

    #[test]
    fn a_typeless_value_list_is_inferred_a_list_property() {
        // Clause 4.5.21.3.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "readings": { "valueList": [1, 2, 3] } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["readings"], json!({ "type": "ListProperty", "valueList": [1, 2, 3] }));
    }

    #[test]
    fn a_typeless_object_list_is_inferred_a_list_relationship() {
        // Clause 4.5.22.3.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "parts": { "objectList": ["urn:ngsi-ld:Part:1"] } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["parts"], json!({ "type": "ListRelationship", "objectList": ["urn:ngsi-ld:Part:1"] }));
    }

    #[test]
    fn a_typeless_json_is_inferred_a_json_property() {
        // Clause 4.5.24.3.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "config": { "json": { "k": 1 } } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["config"], json!({ "type": "JsonProperty", "json": { "k": 1 } }));
    }

    #[test]
    fn a_bare_geometry_is_inferred_a_geo_property() {
        // Clause 4.5.2.3 geospatial exception: a bare GeoJSON geometry is the
        // value of a GeoProperty, not a Property whose value is that object.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "location": { "type": "Point", "coordinates": [13.4, 52.5] } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["location"],
            json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [13.4, 52.5] } })
        );
    }

    #[test]
    fn an_inferred_relationships_sub_attributes_are_still_normalized() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "belongsTo": { "object": "urn:ngsi-ld:Site:1", "reliability": 0.9 }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["belongsTo"]["type"], json!("Relationship"));
        pretty_assertions::assert_eq!(payload["belongsTo"]["object"], json!("urn:ngsi-ld:Site:1"));
        pretty_assertions::assert_eq!(payload["belongsTo"]["reliability"], json!({ "type": "Property", "value": 0.9 }));
    }

    #[test]
    fn a_multi_instance_array_of_typeless_relationships_is_inferred_per_instance() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "belongsTo": [
                { "object": "urn:ngsi-ld:Site:1", "datasetId": "urn:ngsi-ld:Dataset:a" },
                { "object": "urn:ngsi-ld:Site:2", "datasetId": "urn:ngsi-ld:Dataset:b" }
            ]
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["belongsTo"][0]["type"], json!("Relationship"));
        pretty_assertions::assert_eq!(payload["belongsTo"][1]["type"], json!("Relationship"));
    }

    #[test]
    fn an_object_with_no_distinguishing_member_stays_an_implicit_property() {
        // A plain JSON object with no marker is a Property value, unchanged from
        // the pre-inference behaviour.
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "raw": { "foo": 1, "bar": 2 } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["raw"], json!({ "type": "Property", "value": { "foo": 1, "bar": 2 } }));
    }

    #[test]
    fn an_object_with_two_distinguishing_members_is_ambiguous_and_stays_a_property() {
        // Two markers name no single type, so nothing is inferred; the object
        // becomes an implicit Property value rather than a guessed type.
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "ambiguous": { "object": "urn:ngsi-ld:X:1", "languageMap": { "en": "x" } }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["ambiguous"]["type"], json!("Property"));
        assert!(payload["ambiguous"]["value"].is_object());
    }
}
