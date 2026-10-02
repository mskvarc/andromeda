//! The clause 4.5.4 simplified (`keyValues`) rendering of an entity.
//!
//! Two readings of one transformation, kept in one module because they must
//! agree: [`simplify_entity`] walks the typed [`Entity`] and
//! [`simplify_value`] walks an already-serialised
//! representation, which is what the language-negotiated and inline-join paths
//! carry. The typed reading used to live on `Entity` itself, in the
//! specification model crate — a resource does not render itself.

use crate::object_list::bare_uri_object_list;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, rfc3339::sub_second};
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::Entity,
};
use andromeda_ngsi_ld_query::dataset::DEFAULT_INSTANCE_TOKEN;
use andromeda_ngsi_ld_representation::entity_members::ENTITY_MEMBERS;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};
use std::str::FromStr;

/// The clause 4.5.4 simplified rendering of one attribute family's value.
///
/// A `LanguageProperty`, `JsonProperty` and `VocabProperty` each keep their
/// single distinguishing member (`{"languageMap": …}`, `{"json": …}`,
/// `{"vocab": …}`), so the simplified value reads back as the same family
/// rather than as a Property whose value happens to be a map, document or
/// string. A `ListRelationship` is the one family whose simplified value is not
/// the member it was read from: clause 4.5.4 gives it "an ordered array holding
/// the `ListRelationship`'s Objects (represented as URIs)", while the
/// normalized `objectList` those are read from holds the `object`-keyed
/// Relationship Objects clause 4.5.22.2 mandates. Every other family carries
/// its value across bare.
fn simplified_family_value(attr_type: AttributeType, value: Value) -> Value {
    match attr_type {
        AttributeType::LanguageProperty | AttributeType::JsonProperty | AttributeType::VocabProperty => wrap_value_member(attr_type, value),
        AttributeType::ListRelationship => bare_uri_object_list(value),
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::Relationship | AttributeType::ListProperty => value,
    }
}

/// The clause 4.5.4 multi-attribute rendering of one attribute.
///
/// Clause 4.5.4: in the multi-attribute case the value is "a JSON Object
/// containing a single Attribute with a key called `dataset` and its value in
/// turn is a JSON Object holding a series of key-value pairs, one for each
/// datasetId … The default datasetId (where present) is represented by the
/// JSON-LD keyword `@none`."
fn dataset_map_member(datasets: Map<String, Value>) -> Value {
    let mut wrapper = Map::with_capacity(1);
    wrapper.insert(compact_properties::DATASET.to_owned(), Value::Object(datasets));
    Value::Object(wrapper)
}

/// Wrap a family's raw value in its clause 4.5.4 single member.
fn wrap_value_member(attr_type: AttributeType, value: Value) -> Value {
    let mut wrapper = Map::with_capacity(1);
    wrapper.insert(attr_type.compact_value_key().to_string(), value);
    Value::Object(wrapper)
}

/// The clause 4.5.4 simplified value of one typed attribute instance.
fn simplify_instance(attr: &Attribute) -> Value {
    simplified_family_value(attr.attribute_type(), attr.value())
}

/// Render an entity in the clause 4.5.4 simplified (`keyValues`) form.
///
/// Walks the typed [`Entity`] directly, which is what lets the simplified path
/// skip a full serialise round-trip. It lived on `Entity` itself until this
/// module existed: a resource is what the specification defines, and how it is
/// rendered is a decision above it.
#[must_use]
pub fn simplify_entity(entity: &Entity) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), serde_json::json!(entity.id));
    map.insert("type".to_string(), serde_json::json!(entity.type_));

    if let Some(scope) = &entity.scope {
        map.insert("scope".to_string(), serde_json::json!(scope));
    }

    if let Some(ref expires_at) = entity.expires_at {
        map.insert("expiresAt".to_string(), serde_json::json!(sub_second(*expires_at)));
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
        map.insert("createdAt".to_string(), serde_json::json!(sub_second(*created_at)));
    }

    if let Some(ref modified_at) = entity.modified_at {
        map.insert("modifiedAt".to_string(), serde_json::json!(sub_second(*modified_at)));
    }

    if let Some(ref deleted_at) = entity.deleted_at {
        map.insert("deletedAt".to_string(), serde_json::json!(sub_second(*deleted_at)));
    }

    for (key, attr) in &entity.attrs {
        map.insert(key.to_string(), simplify_attribute_value(attr));
    }

    Value::Object(map)
}

/// Simplify a single `AttributeValue` to its key-value form.
fn simplify_attribute_value(attr: &AttributeValue) -> Value {
    match attr {
        AttributeValue::Single(a) => simplify_instance(a),
        AttributeValue::Multiple(list) => {
            // Clause 4.5.4 dataset map: {"dataset": {"<datasetId>": v, "@none": v}}.
            let mut dataset_map = Map::new();
            for attr in list {
                let ds_key = attr.dataset_id().map_or_else(|| DEFAULT_INSTANCE_TOKEN.to_string(), ToString::to_string);
                dataset_map.insert(ds_key, simplify_instance(attr));
            }
            dataset_map_member(dataset_map)
        }
        AttributeValue::Raw(v) => simplify_raw_envelope(v),
    }
}

/// Simplify a raw (already-serialised) attribute envelope that reached the typed
/// path — a language-negotiated value, or a member the broker carries through.
///
/// The value member is read through [`AttributeType::compact_value_key`] rather
/// than a hand-written key list, so a `VocabProperty` reads its `vocab` member
/// rather than a `value` it never carries, and each family is rendered by
/// [`simplified_family_value`].
pub(crate) fn simplify_raw_envelope(envelope: &Value) -> Value {
    // clone: the simplified form is owned and the envelope is only borrowed.
    let Some(obj) = envelope.as_object() else {
        return envelope.clone();
    };
    // The `type` member of a raw envelope is unvalidated JSON, so it is read
    // through `AttributeType`'s own parser; anything it rejects keeps the envelope.
    let Some(attr_type) = obj.get("type").and_then(|kind| kind.as_str()).and_then(|raw| AttributeType::from_str(raw).ok()) else {
        return envelope.clone();
    };
    let Some(value) = obj.get(attr_type.compact_value_key()) else {
        return envelope.clone();
    };
    simplified_family_value(attr_type, value.clone()) // clone: the simplified value is handed back owned
}

/// Render an already-serialised entity in the clause 4.5.4 simplified form.
///
/// The second reading of [`simplify_entity`], over a value rather than over
/// the typed resource. It is what the language-negotiated and `join=inline`
/// paths carry, and what a payload that reached the broker as JSON-LD and was
/// never an [`Entity`] carries. The two must agree, which is why they are one
/// module and one test.
#[must_use]
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
/// nested entity rather than the URI it was resolved from — clause 4.5.23 is
/// applied before clause 4.5.4, so by the time this runs the expansion is a
/// member of the attribute and outranks the plain object reference.
pub(crate) fn simplify_attr_value(value: Value) -> Value {
    match value {
        Value::Object(mut attr_obj) => {
            let attr_type = attr_obj.get("type").and_then(Value::as_str).and_then(|raw| AttributeType::from_str(raw).ok());
            let Some(attr_type) = attr_type else { return Value::Object(attr_obj) };

            match attr_type {
                // Clause 4.5.4, the inline Linked Entity case: a Relationship
                // naming one target answers with that Entity's key-value pairs
                // (EXAMPLE 9), and one naming an array of targets answers with
                // the ordered array of them (EXAMPLE 10). The `entity` member
                // carries whichever shape clause 4.5.23.2 produced, and each
                // Entity in it is simplified in turn.
                AttributeType::Relationship => match attr_obj.remove("entity") {
                    Some(Value::Array(entities)) => Value::Array(entities.into_iter().map(simplify_value).collect()),
                    Some(single) => simplify_value(single),
                    None => take_value_member(attr_obj, attr_type),
                },
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
        Value::Array(items) => {
            // Clause 4.5.4: a multi-instance attribute simplifies to a dataset
            // map keyed by each instance's datasetId (or @none for the default),
            // the same shape the typed reading builds — not a bare array, which
            // is what let one entity render two shapes depending on which path
            // served it.
            let mut dataset_map = Map::with_capacity(items.len());
            for item in items {
                let ds_key = item
                    .as_object()
                    .and_then(|instance| instance.get("datasetId"))
                    .and_then(Value::as_str)
                    .map_or_else(|| DEFAULT_INSTANCE_TOKEN.to_string(), str::to_string);
                dataset_map.insert(ds_key, simplify_attr_value(item));
            }
            dataset_map_member(dataset_map)
        }
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
    }
}

/// Take the member that carries an attribute's value, keeping the clause 4.5.4
/// wrapper for the families that require it, or keep the whole object.
///
/// An attribute whose declared type has no matching value member is not one
/// this rendering understands, and passing it through unchanged is the only
/// answer that does not invent one.
fn take_value_member(mut attr_obj: Map<String, Value>, attr_type: AttributeType) -> Value {
    match attr_obj.remove(attr_type.compact_value_key()) {
        Some(value) => simplified_family_value(attr_type, value),
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

    /// Clause 4.5.4, `ListRelationship` bullet: the simplified rendering is
    /// "an ordered array holding the `ListRelationship`'s Objects (represented
    /// as URIs)", not the `object`-keyed Relationship Objects clause 4.5.22.2
    /// gives the normalized member those are read from.
    #[test]
    fn both_readings_agree_on_a_list_relationship_rendered_as_bare_uris() {
        both_readings_agree(json!({
            "id": "urn:ngsi-ld:Vehicle:4",
            "type": "Vehicle",
            "hasParts": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Part:1", "urn:ngsi-ld:Part:2"]}
        }));
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Vehicle:5",
            "type": "Vehicle",
            "hasParts": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Part:1"}]}
        }));
        pretty_assertions::assert_eq!(simplify_entity(&entity)["hasParts"], json!(["urn:ngsi-ld:Part:1"]));
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
    fn simplified_keeps_the_clause_4_5_4_wrappers_for_language_json_and_vocab() {
        // Clause 4.5.4: these three families keep their single distinguishing
        // member so the simplified value reads back as the same family, not as
        // a Property whose value is a bare map, document or string.
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:2",
            "type": "Building",
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place"}},
            "spec": {"type": "JsonProperty", "json": {"a": 1}},
            "kind": {"type": "VocabProperty", "vocab": "commercial"}
        }));
        let typed = simplify_entity(&entity);
        let serialised = simplify_value(serde_json::to_value(&entity).expect("serialised entity"));
        for rendered in [&typed, &serialised] {
            pretty_assertions::assert_eq!(rendered["street"], json!({"languageMap": {"fr": "Grand Place"}}));
            pretty_assertions::assert_eq!(rendered["spec"], json!({"json": {"a": 1}}));
            pretty_assertions::assert_eq!(rendered["kind"], json!({"vocab": "commercial"}));
        }
    }

    /// Clause 4.5.4, inline Linked Entity retrieval, EXAMPLE 9: a Relationship
    /// naming one target answers with that Entity's key-value pairs.
    #[test]
    fn an_inline_joined_single_target_relationship_simplifies_to_the_nested_entity() {
        let joined = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "providedBy": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Device:31415",
                "entity": {"id": "urn:ngsi-ld:Device:31415", "type": "Device", "brandName": {"type": "Property", "value": "Anemometer"}}
            }
        });

        pretty_assertions::assert_eq!(
            simplify_value(joined)["providedBy"],
            json!({"id": "urn:ngsi-ld:Device:31415", "type": "Device", "brandName": "Anemometer"})
        );
    }

    /// Clause 4.5.4, inline Linked Entity retrieval, EXAMPLE 10: a Relationship
    /// naming an array of targets answers with the ordered array of simplified
    /// Entities, each simplified in its own right.
    #[test]
    fn an_inline_joined_multi_target_relationship_simplifies_every_nested_entity() {
        let joined = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "passengers": {
                "type": "Relationship",
                "objectType": "Person",
                "object": ["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"],
                "entity": [
                    {"id": "urn:ngsi-ld:Person:Alice", "type": "Person", "name": {"type": "Property", "value": "Alice"}},
                    {"id": "urn:ngsi-ld:Person:Bob", "type": "Person", "name": {"type": "Property", "value": "Bob"}}
                ]
            }
        });

        pretty_assertions::assert_eq!(
            simplify_value(joined)["passengers"],
            json!([
                {"id": "urn:ngsi-ld:Person:Alice", "type": "Person", "name": "Alice"},
                {"id": "urn:ngsi-ld:Person:Bob", "type": "Person", "name": "Bob"}
            ])
        );
    }

    #[test]
    fn a_multi_instance_attribute_renders_a_dataset_map_on_both_readings() {
        // Clause 4.5.4: several datasetId-separated instances simplify to a
        // dataset map, keyed by each instance's datasetId (or @none for the
        // default). Both readings must build the same shape.
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Vehicle:9",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 25, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"type": "Property", "value": 30}
            ]
        }));
        let typed = simplify_entity(&entity);
        let serialised = simplify_value(serde_json::to_value(&entity).expect("serialised entity"));
        let expected = json!({"dataset": {"urn:ngsi-ld:Dataset:a": 25, "@none": 30}});
        pretty_assertions::assert_eq!(typed["speed"], expected);
        pretty_assertions::assert_eq!(serialised["speed"], expected);
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
        // Clause 4.5.23 runs before clause 4.5.4, so by the time this sees the
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
    fn a_serialised_vocab_property_simplifies_to_its_vocab_member_only() {
        // Clause 4.5.4: a VocabProperty carries its value in `vocab`, not
        // `value`. Simplifying reads `vocab` and keeps only that family member;
        // the whole envelope (with its `type` and any metadata) must not leak
        // through, which is what looking for a `value` it never carries did.
        let simplified = simplify_value(json!({
            "id": "urn:ngsi-ld:Vehicle:11",
            "type": "Vehicle",
            "kind": {"type": "VocabProperty", "vocab": "commercial", "observedAt": "2025-01-01T00:00:00Z"}
        }));
        pretty_assertions::assert_eq!(simplified["kind"], json!({"vocab": "commercial"}));
        assert!(simplified["kind"].get("type").is_none(), "the VocabProperty envelope must not leak its type");
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
