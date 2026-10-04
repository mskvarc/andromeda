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
//! Where clause 4.5 allows the type to be omitted, it is inferred the way
//! [`crate::concise_reading`] reads the node: an `object` member makes a
//! Relationship, `languageMap` a `LanguageProperty`, `value` a Property, a bare
//! `GeoJSON` geometry a `GeoProperty`, and so on — only a value with no such
//! marker becomes an implicit Property. A node that reading refuses has been
//! refused by [`crate::attribute_type_validation`] before the rewrite runs, and
//! is left as written.
//!
//! The attribute's own name travels with it through the descent, because one
//! of the reserved-member decisions is not a statement about the family alone:
//! clause 4.5.2.2 calls `entityIdSealed` and `entityTypeSealed` "non-reified
//! subproperties" of the `ngsildproof` Property, so under that one name they
//! are envelope members and under every other name they are not members at
//! all. [`crate::proof_seal_admission`] owns both halves of that rule.

use crate::{
    concise_reading::{ArrayReading, ObjectReading, read_array, read_object, value_member_family},
    entity_members::ENTITY_MEMBERS,
    output_only_member::discard_output_only_member,
    proof_seal_admission::is_admitted_proof_seal,
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::{Map, Value};
use std::{mem::take, str::FromStr};

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
        normalize_attribute(member, value);
    }
}

/// Normalize an entity merge fragment while preserving first-level NGSI-LD Null members.
///
/// Clause 5.5.12 assigns a first-level `"urn:ngsi-ld:null"` a different
/// meaning from a concise Property value: it removes the complete target
/// member. Keeping that scalar intact lets JSON-LD expansion retain the
/// distinction for the merge algorithm.
pub fn normalize_merge_fragment(payload: &mut Value) {
    let Some(entity) = payload.as_object_mut() else {
        return;
    };
    for (member, value) in entity.iter_mut() {
        if ENTITY_MEMBERS.contains(&member.as_str()) || value.as_str() == Some("urn:ngsi-ld:null") {
            continue;
        }
        normalize_attribute(member, value);
    }
}

/// Rewrite one bare attribute node named `attribute_name`, as clauses 5.6.4
/// and 5.6.19 supply it.
///
/// Partial Attribute Update and Replace Attribute take an Attribute Fragment
/// rather than an Entity Fragment, so they have no entity envelope to walk and
/// cannot go through [`normalize_entity_attributes`]. Without this they would
/// skip the concise-form normalization every other write path applies, and with
/// it the discarding of the output-only `previous...` member clauses 5.2.5 to
/// 5.2.38 require an implementation to ignore when a Context Producer supplies
/// one.
///
/// The name is the caller's because those two operations carry it in the
/// request URI rather than in the payload, and clause 4.5.2.2 makes the name
/// part of what an instance's members mean: only under `ngsildproof` are the
/// two data-integrity seals envelope members instead of sub-attributes.
pub fn normalize_attribute_node(attribute_name: &str, value: &mut Value) {
    let Some(instance) = value.as_object_mut() else {
        return;
    };
    // The fragment IS the attribute instance, so it is normalized as one. It
    // must not go through the slot logic: that reads a typeless object with no
    // distinguishing member as the concise Property VALUE clause 4.5.2.3
    // abbreviates, and a fragment updating only a sub-attribute -- which carries
    // neither `type` nor a value member -- would be wrapped as a value instead
    // of walked as an instance.
    normalize_concise_instance(attribute_name, instance);
}

/// Rewrite one attribute slot named `attribute_name`: one instance, several, or
/// a concise value, as [`crate::concise_reading`] reads it.
fn normalize_attribute(attribute_name: &str, value: &mut Value) {
    match value {
        Value::Object(instance) => match read_object(instance) {
            // A declared type, or one clause 4.5 infers from the value member
            // it carries, is an attribute instance: name the type where it was
            // omitted, then normalize its sub-attributes, so the reserved
            // members such as `unitCode` stay in the envelope.
            Ok(ObjectReading::Instance(family)) => normalize_inferred_instance(attribute_name, instance, family),
            // The geospatial exception (clause 4.5.2.3): a bare GeoJSON
            // geometry is the value of a GeoProperty, not of a Property.
            Ok(ObjectReading::GeoPropertyValue) => wrap_as_value(value, AttributeType::GeoProperty),
            Ok(ObjectReading::PropertyValue) => wrap_as_value(value, AttributeType::Property),
            Err(_) => {}
        },
        Value::Array(items) => match read_array(items) {
            Ok(ArrayReading::Instances) => {
                for item in items {
                    if let Value::Object(instance) = item
                        && let Ok(ObjectReading::Instance(family)) = read_object(instance)
                    {
                        normalize_inferred_instance(attribute_name, instance, family);
                    }
                }
            }
            Ok(ArrayReading::PropertyValue) => wrap_as_value(value, AttributeType::Property),
            Err(_) => {}
        },
        // A bare scalar is the value of an implicit Property (clause 4.5.2.3).
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => wrap_as_value(value, AttributeType::Property),
    }
}

/// Normalize one instance of `family`, naming the family first where the
/// instance left it to be inferred.
fn normalize_inferred_instance(attribute_name: &str, instance: &mut Map<String, Value>, family: AttributeType) {
    if !instance.contains_key("type") {
        instance.insert("type".to_string(), Value::String(family.to_string()));
    }
    normalize_instance(attribute_name, instance);
}

/// Normalize one instance of the attribute named `attribute_name`, naming its
/// type first if it was left to be inferred from the value member of one of
/// the six families clauses 4.5.3.3 and its siblings let omit it.
///
/// A fragment carrying `value` without a `type` is left untyped: clause 5.6.17
/// gives it the family of the attribute it updates.
fn normalize_concise_instance(attribute_name: &str, instance: &mut Map<String, Value>) {
    if attribute_type_of(instance).is_none()
        && let Ok(Some(inferred)) = value_member_family(instance)
    {
        instance.insert("type".to_string(), Value::String(inferred.to_string()));
    }
    normalize_instance(attribute_name, instance);
}

/// Replace a concise value by the attribute of `family` it abbreviates.
fn wrap_as_value(value: &mut Value, family: AttributeType) {
    let concise = take(value);
    *value = as_typed_value(family, concise);
}

/// The attribute type an instance declares, if it declares one.
fn attribute_type_of(instance: &Map<String, Value>) -> Option<AttributeType> {
    AttributeType::from_str(instance.get("type")?.as_str()?).ok()
}

/// Rewrite the sub-attributes of one instance, leaving its reserved members
/// alone.
///
/// The family's own output-only member goes first: clause 5.2.5 and its seven
/// siblings say a `previous…` a Context Producer supplied "shall be ignored",
/// so it is dropped here rather than normalized into a sub-attribute and
/// stored.
fn normalize_instance(attribute_name: &str, instance: &mut Map<String, Value>) {
    let Some(declared) = attribute_type_of(instance) else {
        return;
    };
    discard_output_only_member(instance, declared);
    let reserved = declared.reserved_fields();
    for (member, value) in instance.iter_mut() {
        if member == "type" || reserved.contains(&member.as_str()) {
            continue;
        }
        // Clause 4.5.2.2 calls `entityIdSealed` and `entityTypeSealed`
        // "non-reified subproperties" of `ngsildproof`, so under that name each
        // carries its value directly and wrapping one as a Property would store
        // the reified shape the clause rules out. Under any other name the same
        // clause forbids them outright, and `crate::attribute_type_validation`
        // has already refused the payload before it reaches here.
        if is_admitted_proof_seal(attribute_name, declared, member) {
            continue;
        }
        normalize_attribute(member, value);
    }
}

/// Wrap a concise value as the `{type, value}` attribute clause 4.5.2.3 says it
/// abbreviates: an implicit Property, or a `GeoProperty` for a bare geometry.
fn as_typed_value(attribute_type: AttributeType, value: Value) -> Value {
    let mut attribute = Map::with_capacity(2);
    attribute.insert("type".to_string(), Value::String(attribute_type.to_string()));
    attribute.insert("value".to_string(), value);
    Value::Object(attribute)
}

#[cfg(test)]
mod tests {
    use crate::normalization::{normalize_attribute_node, normalize_entity_attributes, normalize_merge_fragment};
    use serde_json::json;

    #[test]
    fn a_concise_scalar_becomes_a_property_with_that_value() {
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "speed": 42 });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"], json!({ "type": "Property", "value": 42 }));
    }

    #[test]
    fn a_merge_fragment_preserves_a_first_level_null_sentinel() {
        let mut payload = json!({ "name": "urn:ngsi-ld:null", "speed": 42 });

        normalize_merge_fragment(&mut payload);

        pretty_assertions::assert_eq!(payload["name"], json!("urn:ngsi-ld:null"));
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
    fn plural_temporal_values_are_not_normalized_as_a_sub_attribute() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": {
                "type": "Property",
                "values": [[42, "2024-01-01T00:00:00Z"], [43, "2024-01-01T01:00:00Z"]]
            }
        });
        let untouched = payload.clone(); // clone: the expectation compares against the input.

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload, untouched);
    }

    /// Clause 5.2.5: `previousValue` is read-only and "if provided in update
    /// or create operations shall be ignored", so it is dropped rather than
    /// wrapped into a sub-attribute and carried into the store.
    #[test]
    fn a_supplied_output_only_member_is_dropped_rather_than_normalized() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": { "type": "Property", "value": 42, "previousValue": 9 },
            "belongsTo": { "object": "urn:ngsi-ld:Site:1", "previousObject": "urn:ngsi-ld:Site:0" }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"], json!({ "type": "Property", "value": 42 }));
        pretty_assertions::assert_eq!(payload["belongsTo"], json!({ "type": "Relationship", "object": "urn:ngsi-ld:Site:1" }));
    }

    /// The same holds one level down: a sub-attribute is an attribute
    /// (clause 4.5.2.2), so its own output-only member is ignored too.
    #[test]
    fn a_sub_attributes_output_only_member_is_dropped_as_well() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": { "type": "Property", "value": 42, "accuracy": { "type": "Property", "value": 0.5, "previousValue": 0.9 } }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"]["accuracy"], json!({ "type": "Property", "value": 0.5 }));
    }

    /// A `previousValue` inside a Property's value is part of that value, not
    /// a member of the instance, so it is preserved verbatim.
    #[test]
    fn an_output_only_name_inside_a_property_value_is_left_alone() {
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "raw": { "previousValue": 9, "other": 1 } });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["raw"], json!({ "type": "Property", "value": { "previousValue": 9, "other": 1 } }));
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

    /// Clause 4.5.2.2: `ngsildproof` is "a Property with the non-reified
    /// subproperties `entityIdSealed` and `entityTypeSealed`". Non-reified
    /// means each carries its value directly, so neither is wrapped into the
    /// `{"type": "Property", "value": …}` envelope every other member of an
    /// instance is wrapped into.
    #[test]
    fn the_two_seals_of_the_ngsildproof_property_are_left_unreified() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": {
                "type": "Property",
                "value": {"streetAddress": "Tiger Street 4"},
                "ngsildproof": {
                    "type": "Property",
                    "entityIdSealed": "urn:ngsi-ld:Store:002",
                    "entityTypeSealed": "Store",
                    "value": {"type": "DataIntegrityProof", "proofValue": "z3XrH3"}
                }
            }
        });

        normalize_entity_attributes(&mut payload);

        let proof = &payload["address"]["ngsildproof"];
        pretty_assertions::assert_eq!(proof["entityIdSealed"], json!("urn:ngsi-ld:Store:002"));
        pretty_assertions::assert_eq!(proof["entityTypeSealed"], json!("Store"));
        pretty_assertions::assert_eq!(proof["value"], json!({"type": "DataIntegrityProof", "proofValue": "z3XrH3"}));
    }

    /// The exception belongs to the name, not to the member: under any other
    /// attribute name the same two members are ordinary sub-attributes and are
    /// wrapped like any other. Reaching that shape at all takes a payload
    /// `crate::attribute_type_validation` has already refused, so this pins the
    /// rewrite's own reading rather than an admissible input.
    #[test]
    fn a_seal_named_member_under_another_attribute_is_wrapped_like_any_sub_attribute() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": {"type": "Property", "value": 42, "entityIdSealed": "urn:ngsi-ld:Device:1"}
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"]["entityIdSealed"], json!({"type": "Property", "value": "urn:ngsi-ld:Device:1"}));
    }

    /// Clauses 4.5.3.2 and 4.5.3.3 prohibit both seals on a Relationship with
    /// no exception, so the `ngsildproof` name alone does not make them
    /// envelope members: clause 4.5.2.2 defines `ngsildproof` as a Property.
    #[test]
    fn a_seal_under_an_ngsildproof_that_is_a_relationship_is_not_an_envelope_member() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "ngsildproof": {"type": "Relationship", "object": "urn:ngsi-ld:Proof:1", "entityTypeSealed": "Device"}
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["ngsildproof"]["entityTypeSealed"], json!({"type": "Property", "value": "Device"}));
    }

    /// Clauses 5.6.4 and 5.6.19 carry the attribute's name in the request URI,
    /// so the Attribute Fragment entry point is told the name rather than
    /// reading it from the payload.
    #[test]
    fn an_attribute_fragment_named_ngsildproof_keeps_its_seals_unreified() {
        let mut fragment = json!({
            "type": "Property",
            "entityIdSealed": "urn:ngsi-ld:Store:002",
            "entityTypeSealed": "Store",
            "value": {"type": "DataIntegrityProof", "proofValue": "z3XrH3"}
        });

        normalize_attribute_node("ngsildproof", &mut fragment);

        pretty_assertions::assert_eq!(fragment["entityIdSealed"], json!("urn:ngsi-ld:Store:002"));
        pretty_assertions::assert_eq!(fragment["entityTypeSealed"], json!("Store"));
    }

    /// The same fragment under another name gets the ordinary treatment, which
    /// is what makes the name and not the payload the deciding input.
    #[test]
    fn an_attribute_fragment_named_otherwise_wraps_the_same_members() {
        let mut fragment = json!({"type": "Property", "value": 42, "entityTypeSealed": "Device"});

        normalize_attribute_node("speed", &mut fragment);

        pretty_assertions::assert_eq!(fragment["entityTypeSealed"], json!({"type": "Property", "value": "Device"}));
    }

    /// A member named like a seal inside a Property's value is part of that
    /// value, so it is preserved verbatim whatever the attribute is called.
    #[test]
    fn a_seal_name_inside_the_ngsildproof_value_is_left_untouched() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "ngsildproof": {"type": "Property", "value": {"entityIdSealed": "urn:ngsi-ld:Store:002"}}
        });
        let untouched = payload.clone(); // clone: the expectation compares against the input

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload, untouched);
    }

    /// Every instance of a multi-instance `ngsildproof` is read the same way,
    /// because the name is the attribute's and not one instance's.
    #[test]
    fn every_instance_of_a_multi_instance_ngsildproof_keeps_its_seals_unreified() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": {
                "type": "Property",
                "value": 1,
                "ngsildproof": [
                    {"type": "Property", "entityIdSealed": "urn:ngsi-ld:Store:002", "value": {"proofValue": "zA"}, "datasetId": "urn:ngsi-ld:Dataset:a"},
                    {"type": "Property", "entityTypeSealed": "Store", "value": {"proofValue": "zB"}, "datasetId": "urn:ngsi-ld:Dataset:b"}
                ]
            }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["address"]["ngsildproof"][0]["entityIdSealed"], json!("urn:ngsi-ld:Store:002"));
        pretty_assertions::assert_eq!(payload["address"]["ngsildproof"][1]["entityTypeSealed"], json!("Store"));
    }

    /// A merge fragment goes through the same rewrite, so clause 5.5.12 does
    /// not reintroduce the reification the other write paths avoid.
    #[test]
    fn a_merge_fragment_keeps_the_seals_of_an_ngsildproof_unreified() {
        let mut payload = json!({
            "address": {
                "type": "Property",
                "value": 1,
                "ngsildproof": {"type": "Property", "entityIdSealed": "urn:ngsi-ld:Store:002", "value": {"proofValue": "zA"}}
            }
        });

        normalize_merge_fragment(&mut payload);

        pretty_assertions::assert_eq!(payload["address"]["ngsildproof"]["entityIdSealed"], json!("urn:ngsi-ld:Store:002"));
    }

    /// Two value members name no single family. The validation refuses the
    /// node, so the rewrite neither guesses a family nor buries the node in an
    /// implicit Property value.
    #[test]
    fn an_object_with_two_value_members_is_left_for_the_validation_to_refuse() {
        let ambiguous = json!({ "object": "urn:ngsi-ld:X:1", "languageMap": { "en": "x" } });
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "ambiguous": ambiguous });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["ambiguous"], ambiguous);
    }

    /// Clause 4.5.5 writes the instances of a multi-attribute as an array, and
    /// clause 4.5.2.3 infers a Property from the `value` each one carries, so a
    /// concise multi-attribute is two instances rather than one array value.
    #[test]
    fn typeless_instances_in_an_array_are_normalized_one_by_one() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "speed": [
                { "value": 25, "datasetId": "urn:ngsi-ld:Dataset:a" },
                { "value": 30, "datasetId": "urn:ngsi-ld:Dataset:b", "accuracy": 0.5 }
            ]
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["speed"],
            json!([
                { "type": "Property", "value": 25, "datasetId": "urn:ngsi-ld:Dataset:a" },
                { "type": "Property", "value": 30, "datasetId": "urn:ngsi-ld:Dataset:b", "accuracy": { "type": "Property", "value": 0.5 } }
            ])
        );
    }

    /// An array mixing instances with values is neither, and the validation
    /// refuses it; the rewrite leaves it as written.
    #[test]
    fn an_array_mixing_instances_and_values_is_left_for_the_validation_to_refuse() {
        let mixed = json!([{ "value": 25, "datasetId": "urn:ngsi-ld:Dataset:a" }, 30]);
        let mut payload = json!({ "id": "urn:ngsi-ld:Device:1", "type": "Device", "speed": mixed });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["speed"], mixed);
    }

    /// Clause 4.5.2.3: a concise Property may carry sub-attributes beside its
    /// `value`. A typeless `{"value": …, "<sub>": …}` is that shape, so the
    /// value stays scalar and the sibling becomes its own Property sub-attribute
    /// rather than being buried inside the Property's value.
    #[test]
    fn a_typeless_value_with_a_sibling_is_a_property_carrying_a_sub_attribute() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "temperature": { "value": 42, "accuracy": 0.5 }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["temperature"],
            json!({ "type": "Property", "value": 42, "accuracy": { "type": "Property", "value": 0.5 } })
        );
    }

    /// A reserved member (`unitCode`) beside a `value` stays in the Property
    /// envelope where clause 4.5.2.2 puts it, rather than being wrapped as a
    /// sub-attribute or buried inside the value.
    #[test]
    fn a_typeless_value_with_a_unit_code_keeps_the_unit_code_in_the_envelope() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "temperature": { "value": 42, "unitCode": "CEL" }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["temperature"], json!({ "type": "Property", "value": 42, "unitCode": "CEL" }));
    }

    /// The inference recurses: a sibling that is itself a concise value with its
    /// own sub-attribute is normalized the same way, one level down.
    #[test]
    fn a_typeless_value_with_a_nested_sub_attribute_recurses() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "temperature": { "value": 42, "accuracy": { "value": 0.5, "confidence": 0.9 } }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["temperature"],
            json!({
                "type": "Property",
                "value": 42,
                "accuracy": { "type": "Property", "value": 0.5, "confidence": { "type": "Property", "value": 0.9 } }
            })
        );
    }

    /// A typeless attribute of a non-Property family carrying a sibling is
    /// inferred from its own distinguishing member (`valueList` -> `ListProperty`),
    /// not from the `value` branch, so its reserved members stay in the envelope
    /// and the sibling becomes a first-class sub-attribute. The `value`-branch
    /// inference is reached only when no distinguishing member is present, so it
    /// cannot mis-type these families.
    #[test]
    fn a_typeless_list_property_with_a_sibling_keeps_its_family_and_normalizes_the_sibling() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "readings": { "valueList": [1, 2, 3], "unitCode": "CEL", "accuracy": 0.5 }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["readings"],
            json!({
                "type": "ListProperty",
                "valueList": [1, 2, 3],
                "unitCode": "CEL",
                "accuracy": { "type": "Property", "value": 0.5 }
            })
        );
    }

    /// Clause 4.5.2.3 geospatial exception: when a typeless concise attribute's
    /// `value` member resolves to a `GeoJSON` geometry, the `GeoProperty` sub-type
    /// is inferred even though a sibling forced the `value`-with-sub-attributes
    /// shape -- a moving entity's `location` with an `observedAt` is the common
    /// case. The sibling still becomes a first-class sub-attribute.
    #[test]
    fn a_typeless_value_resolving_to_a_geometry_with_a_sibling_is_a_geo_property() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "location": { "value": {"type": "Point", "coordinates": [13.4, 52.5]}, "observedAt": "2024-01-01T00:00:00Z" }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["location"],
            json!({
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.4, 52.5]},
                "observedAt": "2024-01-01T00:00:00Z"
            })
        );
    }

    /// Clause 4.5.2.3, "type": "If missing, "Property" can be inferred by the
    /// presence of the "value" attribute" -- with or without a sibling, so a
    /// lone `value` is the Property's value and a `value` nested inside it is
    /// part of that value.
    #[test]
    fn a_lone_value_member_is_the_property_value() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "lone": { "value": 42 },
            "nested": { "value": { "value": 5 } }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["lone"], json!({ "type": "Property", "value": 42 }));
        pretty_assertions::assert_eq!(payload["nested"], json!({ "type": "Property", "value": { "value": 5 } }));
    }

    /// The geospatial exception of the same clause holds for a lone `value`.
    #[test]
    fn a_lone_geometry_value_member_is_a_geo_property() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "location": { "value": {"type": "Point", "coordinates": [13.4, 52.5]} }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(
            payload["location"],
            json!({ "type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]} })
        );
    }
}
