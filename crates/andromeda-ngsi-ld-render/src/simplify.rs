//! The clause 4.5.5 simplified (`keyValues`) rendering of an entity.
//!
//! Two readings of one transformation, kept in one module because they must
//! agree: [`simplify_entity`] walks the typed [`Entity`] and
//! [`simplify_value`] walks an already-serialised
//! representation, which is what the language-negotiated and inline-join paths
//! carry. The typed reading used to live on `Entity` itself, in the
//! specification model crate — a resource does not render itself.

use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, rfc3339::whole_seconds};
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use serde_json::{Map, Value};
use std::str::FromStr;

/// Entity members that are not attributes and pass through untouched.
const ENTITY_MEMBERS: &[&str] = &["id", "type", "scope", "@context"];

/// Render an entity in the clause 4.5.5 simplified (`keyValues`) form.
///
/// Walks the typed [`Entity`] directly, which is what lets the simplified path
/// skip a full serialise round-trip. It lived on `Entity` itself until this
/// module existed: a resource is what the specification defines, and how it is
/// rendered is a decision above it.
pub fn simplify_entity(entity: &Entity) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), serde_json::json!(entity.id));
    map.insert("type".to_string(), serde_json::json!(entity.type_));

    if let Some(scope) = &entity.scope {
        map.insert("scope".to_string(), serde_json::json!(scope));
    }

    if let Some(ref expires_at) = entity.expires_at {
        map.insert("expiresAt".to_string(), serde_json::json!(whole_seconds(*expires_at)));
    }

    if let Some(ref location) = entity.location {
        map.insert("location".to_string(), simplify_attribute_value(location));
    }

    if let Some(ref observation_space) = entity.observation_space {
        map.insert("observationSpace".to_string(), simplify_attribute_value(observation_space));
    }

    if let Some(ref operation_space) = entity.operation_space {
        map.insert("operationSpace".to_string(), simplify_attribute_value(operation_space));
    }

    if let Some(ref created_at) = entity.created_at {
        map.insert("createdAt".to_string(), serde_json::json!(whole_seconds(*created_at)));
    }

    if let Some(ref modified_at) = entity.modified_at {
        map.insert("modifiedAt".to_string(), serde_json::json!(whole_seconds(*modified_at)));
    }

    if let Some(ref deleted_at) = entity.deleted_at {
        map.insert("deletedAt".to_string(), serde_json::json!(whole_seconds(*deleted_at)));
    }

    for (key, attr) in &entity.attrs {
        map.insert(key.to_string(), simplify_attribute_value(attr));
    }

    Value::Object(map)
}

/// Simplify a single `AttributeValue` to its key-value form.
fn simplify_attribute_value(attr: &AttributeValue) -> Value {
    match attr {
        AttributeValue::Single(a) => a.value(),
        AttributeValue::Multiple(list) => {
            // Dataset map format: {"dataset": {"datasetId1": value1, "@none": value2}}
            let mut dataset_map = Map::new();
            for attr in list {
                let ds_key = attr.dataset_id_string().unwrap_or_else(|| "@none".to_string());
                dataset_map.insert(ds_key, attr.value());
            }
            serde_json::json!({"dataset": dataset_map})
        }
        AttributeValue::Raw(v) => {
            // For language-negotiated or other raw values that still
            // have an NGSI-LD attribute envelope, extract the value.
            // clone: the simplified form is an owned `Value` and the attribute is
            // only borrowed, so every arm below hands back a copy of either the
            // payload member or the whole envelope.
            if let Some(obj) = v.as_object() {
                // The `type` member of a raw envelope is unvalidated JSON, so it
                // is read through `AttributeType`'s own parser; anything it rejects
                // keeps the envelope.
                match obj.get("type").and_then(|kind| kind.as_str()).and_then(|raw| AttributeType::from_str(raw).ok()) {
                    Some(AttributeType::Property | AttributeType::GeoProperty | AttributeType::VocabProperty) => {
                        obj.get("value").cloned().unwrap_or_else(|| v.clone())
                    }
                    Some(AttributeType::Relationship) => obj.get("object").cloned().unwrap_or_else(|| v.clone()),
                    Some(AttributeType::ListProperty) => obj.get("valueList").cloned().unwrap_or_else(|| v.clone()),
                    Some(AttributeType::ListRelationship) => obj.get("objectList").cloned().unwrap_or_else(|| v.clone()),
                    // A language map and a JSON document are already their own
                    // value, and a `type` the parser rejects is not an attribute
                    // envelope at all.
                    Some(AttributeType::LanguageProperty | AttributeType::JsonProperty) | None => v.clone(),
                }
            } else {
                // clone: a raw member that is not an envelope is handed back whole.
                v.clone()
            }
        }
    }
}

/// Render an already-serialised entity in the clause 4.5.4 simplified form.
///
/// The second reading of [`simplify_entity`], over a value rather than over
/// the typed resource. It is what the language-negotiated and `join=inline`
/// paths carry, and what a payload that reached the broker as JSON-LD and was
/// never an [`Entity`] carries. The two must agree, which is why they are one
/// module and one test.
pub fn simplify_value(val: Value) -> Value {
    let Value::Object(obj) = val else { return val };

    let mut map = Map::with_capacity(obj.len());
    for (key, value) in obj {
        if ENTITY_MEMBERS.contains(&key.as_str()) {
            map.insert(key, value);
        } else {
            map.insert(key, simplify_attr_value(value));
        }
    }
    Value::Object(map)
}

/// Simplify one attribute value of an already-serialised entity.
///
/// A Relationship that `join=inline` has already expanded answers with the
/// nested entity rather than the URI it was resolved from — clause 4.11 is
/// applied before clause 4.5.4, so by the time this runs the expansion is a
/// member of the attribute and outranks the plain object reference.
fn simplify_attr_value(value: Value) -> Value {
    match value {
        Value::Object(mut attr_obj) => {
            let attr_type = attr_obj.get("type").and_then(Value::as_str).and_then(|raw| AttributeType::from_str(raw).ok());
            let Some(attr_type) = attr_type else { return Value::Object(attr_obj) };

            match attr_type {
                AttributeType::Relationship => attr_obj.remove("entity").map_or_else(|| take_value_member(attr_obj, attr_type), simplify_value),
                AttributeType::ListRelationship => match attr_obj.remove("entityList") {
                    Some(Value::Array(items)) => Value::Array(items.into_iter().map(simplify_value).collect()),
                    Some(other) => other,
                    None => take_value_member(attr_obj, attr_type),
                },
                AttributeType::Property
                | AttributeType::GeoProperty
                | AttributeType::VocabProperty
                | AttributeType::LanguageProperty
                | AttributeType::JsonProperty
                | AttributeType::ListProperty => take_value_member(attr_obj, attr_type),
            }
        }
        Value::Array(items) => Value::Array(items.into_iter().map(simplify_attr_value).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
    }
}

/// Take the member that carries an attribute's value, or keep the whole object.
///
/// An attribute whose declared type has no matching value member is not one
/// this rendering understands, and passing it through unchanged is the only
/// answer that does not invent one.
fn take_value_member(mut attr_obj: Map<String, Value>, attr_type: AttributeType) -> Value {
    match attr_obj.remove(attr_type.compact_value_key()) {
        Some(value) => value,
        None => Value::Object(attr_obj),
    }
}

#[cfg(test)]
mod tests {
    use crate::simplify::{simplify_entity, simplify_value};
    use andromeda_ngsi_ld_model::entity::Entity;
    use serde_json::{Value, json};

    fn parse_entity(value: Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    /// Assert that both readings of clause 4.5.4 answer identically.
    ///
    /// This is the claim the module doc-comment makes and that nothing checked
    /// while the two lived in different files: `simplify_entity` walks the
    /// typed resource for the fast path, `simplify_value` walks the serialised
    /// one for the language-negotiated and `join=inline` paths, and a request
    /// takes one or the other depending on parameters that have nothing to do
    /// with what the answer should be.
    fn both_readings_agree(entity: Value) {
        let entity = parse_entity(entity);
        let serialised = serde_json::to_value(&entity).expect("serialised entity");
        pretty_assertions::assert_eq!(simplify_entity(&entity), simplify_value(serialised));
    }

    #[test]
    fn both_readings_agree_on_a_plain_property() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 25}
        }));
    }

    #[test]
    fn both_readings_agree_on_a_relationship() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Vehicle:2",
            "type": "Vehicle",
            "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:1"}
        }));
    }

    #[test]
    fn both_readings_agree_on_a_geo_property() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Vehicle:3",
            "type": "Vehicle",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}}
        }));
    }

    #[test]
    fn both_readings_agree_on_a_language_property() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}}
        }));
    }

    #[test]
    fn both_readings_agree_on_the_list_and_vocab_shapes() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Vehicle:4",
            "type": "Vehicle",
            "tags": {"type": "VocabProperty", "vocab": ["urn:ngsi-ld:Tag:1"]},
            "readings": {"type": "ListProperty", "valueList": [1, 2, 3]},
            "parts": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Part:1"]},
            "spec": {"type": "JsonProperty", "json": {"a": 1}}
        }));
    }

    #[test]
    fn both_readings_agree_on_an_attribute_carrying_metadata_and_sub_attributes() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Vehicle:5",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "value": 25,
                "unitCode": "KMH",
                "observedAt": "2025-01-01T00:00:00Z",
                "accuracy": {"type": "Property", "value": 0.95}
            }
        }));
    }

    #[test]
    fn both_readings_agree_on_the_entity_members_that_are_not_attributes() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Vehicle:6",
            "type": "Vehicle",
            "scope": "/Madrid/Gardens",
            "speed": {"type": "Property", "value": 25}
        }));
    }

    #[test]
    fn an_inline_joined_relationship_answers_with_the_nested_entity() {
        // Clause 4.11 runs before clause 4.5.4, so by the time this sees the
        // attribute the expansion is already a member and outranks the URI.
        let joined = json!({
            "id": "urn:ngsi-ld:Vehicle:7",
            "type": "Vehicle",
            "isParked": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:OffStreetParking:1",
                "entity": {
                    "id": "urn:ngsi-ld:OffStreetParking:1",
                    "type": "OffStreetParking",
                    "name": {"type": "Property", "value": "Downtown"}
                }
            }
        });
        let simplified = simplify_value(joined);
        pretty_assertions::assert_eq!(simplified["isParked"]["id"], "urn:ngsi-ld:OffStreetParking:1");
        pretty_assertions::assert_eq!(simplified["isParked"]["name"], "Downtown");
    }

    #[test]
    fn an_attribute_with_an_unrecognised_type_keeps_its_envelope() {
        let simplified = simplify_value(json!({
            "id": "urn:ngsi-ld:Vehicle:8",
            "type": "Vehicle",
            "odd": {"type": "NotAnAttributeType", "value": 1}
        }));
        pretty_assertions::assert_eq!(simplified["odd"], json!({"type": "NotAnAttributeType", "value": 1}));
    }
}
