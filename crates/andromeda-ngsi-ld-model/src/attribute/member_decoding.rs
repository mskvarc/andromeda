//! Decoding one attribute member of an entity into an [`AttributeValue`].
//!
//! A member is one attribute instance, an array of them, or a value carried
//! through verbatim, tried in that order. The derived decoding of that choice
//! (`#[serde(untagged)]`) buffers the whole member before trying a variant, and
//! the internally tagged [`Attribute`] it tries first buffers it once more to
//! find the `type` tag: a `GeoProperty` of a million positions crossed serde's
//! buffered content three times over, close to a gigabyte for one entity. The
//! member is therefore read once into a JSON tree, each instance is decoded
//! from that tree, and a `GeoProperty`'s geometry is read from it in place.
//! The result is the one the derived decoding gives, with one deliberate
//! exception: a geometry carrying a JSON-LD keyword member, such as an
//! `@context`, is not read as a typed geometry, so the member stays as written
//! for the clause 5.5.7 screening that refuses it.

use crate::attribute::{
    geo_property::GeoProperty,
    instance::{Attribute, AttributeValue},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

// Hand-written because the derived `untagged` decoding is the buffering this
// module exists to avoid; see the module documentation.
impl<'de> Deserialize<'de> for AttributeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<AttributeValue, D::Error> {
        Value::deserialize(deserializer).map(AttributeValue::from_member)
    }
}

impl AttributeValue {
    /// Read one attribute member: an instance, an array of instances, or,
    /// when it is neither, the member as it stands.
    #[must_use]
    pub fn from_member(member: Value) -> AttributeValue {
        match member {
            // Only an object can be one instance, and an object is no array of
            // them.
            Value::Object(_) => match attribute_of(&member) {
                Ok(instance) => AttributeValue::Single(Box::new(instance)),
                Err(_) => AttributeValue::Raw(member),
            },
            Value::Array(items) => match items.iter().map(attribute_of).collect::<Result<Vec<Attribute>, serde_json::Error>>() {
                Ok(instances) => AttributeValue::Multiple(instances),
                Err(_) => AttributeValue::Raw(Value::Array(items)),
            },
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => AttributeValue::Raw(member),
        }
    }
}

/// Decode one attribute instance, a `GeoProperty` by its geometry-aware reader
/// and every other family as the derived decoding reads it.
fn attribute_of(instance: &Value) -> Result<Attribute, serde_json::Error> {
    match instance.as_object() {
        Some(members) if members.get("type").and_then(Value::as_str) == Some(AttributeType::GeoProperty.short_name()) => {
            GeoProperty::from_instance(members).map(Attribute::GeoProperty)
        }
        Some(_) | None => deserialize_exact(instance),
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::{Attribute, AttributeValue};
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};

    /// The decoding `AttributeValue` derived before it was written by hand:
    /// each variant tried in order over a buffered copy of the member. Every
    /// member must decode to the same variant holding the same content.
    #[derive(Debug, Serialize, Deserialize)]
    #[serde(untagged)]
    enum DerivedAttributeValue {
        Single(Box<Attribute>),
        Multiple(Vec<Attribute>),
        Raw(Value),
    }

    /// Which variant a decoding chose, and what it holds rendered back to JSON.
    #[derive(Debug, PartialEq)]
    struct Decoded {
        variant: &'static str,
        content: Value,
    }

    fn hand_written(value: &AttributeValue) -> Decoded {
        let variant = match value {
            AttributeValue::Single(_) => "single",
            AttributeValue::Multiple(_) => "multiple",
            AttributeValue::Raw(_) => "raw",
        };
        Decoded {
            variant,
            content: serde_json::to_value(value).expect("an attribute member renders"),
        }
    }

    fn derived(value: &DerivedAttributeValue) -> Decoded {
        let variant = match value {
            DerivedAttributeValue::Single(_) => "single",
            DerivedAttributeValue::Multiple(_) => "multiple",
            DerivedAttributeValue::Raw(_) => "raw",
        };
        Decoded {
            variant,
            content: serde_json::to_value(value).expect("an attribute member renders"),
        }
    }

    /// Decode `text` both ways, from the request text as an entity is decoded
    /// and from its parsed tree, assert the three agree, and answer the variant.
    fn decoded_alike(text: &str) -> &'static str {
        let from_text = hand_written(&serde_json::from_str::<AttributeValue>(text).expect("the member decodes"));
        let from_tree = hand_written(&AttributeValue::from_member(serde_json::from_str(text).expect("the member parses")));
        let oracle = derived(&serde_json::from_str::<DerivedAttributeValue>(text).expect("the derived decoding reads the member"));
        assert_eq!(from_text, oracle, "{text}");
        assert_eq!(from_tree, oracle, "{text}");
        oracle.variant
    }

    #[test]
    fn a_single_property_decodes_as_the_derived_decoding_does() {
        let text = r#"{"type": "Property", "value": 0.100000000000000005, "unitCode": "CEL", "observedAt": "2024-01-01T00:00:00Z", "accuracy": {"type": "Property", "value": 0.5}}"#;
        assert_eq!(decoded_alike(text), "single");
    }

    #[test]
    fn a_geo_property_with_an_object_geometry_decodes_as_the_derived_decoding_does() {
        let text = json!({
            "type": "GeoProperty",
            "value": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0, 12.5], [0.0, 0.0]]]},
            "previousValue": {"type": "Point", "coordinates": [1.0, 2.0]},
            "datasetId": "urn:ngsi-ld:Dataset:a",
            "valueType": "geojson:Polygon",
            "observedAt": "2024-01-01T00:00:00Z",
            "accuracy": {"type": "Property", "value": 3}
        })
        .to_string();
        assert_eq!(decoded_alike(&text), "single");
    }

    #[test]
    fn a_geo_property_with_a_null_previous_value_decodes_as_the_derived_decoding_does() {
        let text = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "previousValue": null}"#;
        assert_eq!(decoded_alike(text), "single");
    }

    /// Clause 4.7.2 lets a geometry travel as a JSON string; the domain type
    /// holds a decoded geometry, so the member is carried verbatim as it
    /// always was.
    #[test]
    fn a_geo_property_with_a_string_encoded_geometry_decodes_as_the_derived_decoding_does() {
        let text = json!({"type": "GeoProperty", "value": json!({"type": "Point", "coordinates": [1.0, 2.0]}).to_string()}).to_string();
        assert_eq!(decoded_alike(&text), "raw");
    }

    #[test]
    fn a_geo_property_with_a_malformed_geometry_decodes_as_the_derived_decoding_does() {
        for geometry in [
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [0.0, 0.0]]]}),
            json!({"type": "Point", "coordinates": [181.0, 2.0]}),
            json!({"type": "Point"}),
            json!({"type": "GeometryCollection", "geometries": []}),
        ] {
            let text = json!({"type": "GeoProperty", "value": geometry}).to_string();
            assert_eq!(decoded_alike(&text), "raw");
        }
        assert_eq!(decoded_alike(r#"{"type": "GeoProperty"}"#), "raw");
    }

    #[test]
    fn a_multi_instance_array_decodes_as_the_derived_decoding_does() {
        let text = json!([
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
            {"type": "GeoProperty", "datasetId": "urn:ngsi-ld:Dataset:b", "value": {"type": "Point", "coordinates": [3.0, 4.0]}},
            {"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:c", "value": 5}
        ])
        .to_string();
        assert_eq!(decoded_alike(&text), "multiple");
        assert_eq!(decoded_alike("[]"), "multiple");
    }

    #[test]
    fn an_array_holding_a_non_instance_decodes_as_the_derived_decoding_does() {
        let text = json!([{"type": "Property", "value": 1}, {"no": "type"}]).to_string();
        assert_eq!(decoded_alike(&text), "raw");
        let text = json!([{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}, {"type": "GeoProperty", "value": "not a geometry"}])
            .to_string();
        assert_eq!(decoded_alike(&text), "raw");
        assert_eq!(decoded_alike("[1, 2, 3]"), "raw");
    }

    #[test]
    fn a_scalar_member_decodes_as_the_derived_decoding_does() {
        for text in ["42", "0.100000000000000005", r#""text""#, "true", "null"] {
            assert_eq!(decoded_alike(text), "raw");
        }
    }

    #[test]
    fn an_object_that_is_no_instance_decodes_as_the_derived_decoding_does() {
        for text in [
            r#"{"value": 1}"#,
            r#"{"type": "SomethingElse", "value": 1}"#,
            r#"{"type": "https://uri.etsi.org/ngsi-ld/GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}"#,
            r#"{"type": 7}"#,
            r#"{"type": "Relationship"}"#,
        ] {
            assert_eq!(decoded_alike(text), "raw");
        }
    }
}
