//! Decoding one attribute member of an entity into an [`AttributeValue`].
//!
//! A member is one attribute instance, an array of them, or a value carried
//! through verbatim, tried in that order. The derived decoding of that choice
//! (`#[serde(untagged)]`) buffers the whole member before trying a variant, and
//! the internally tagged [`Attribute`] it tries first buffers it once more to
//! find the `type` tag: a `GeoProperty` of a million positions crossed serde's
//! buffered content three times over, close to a gigabyte for one entity.
//!
//! A member is therefore read in one of two ways, both giving the result the
//! derived decoding gives. Decoded from any deserializer, it is read once into
//! a JSON tree and read from that tree by the model's one reading of a compact
//! node ([`AttributeValue::from_node`]), which a compacted entity's members
//! are read by as well. Decoded from the text of a request, it is kept as the
//! slice of text it is: each instance is decoded straight from that text, a
//! `GeoProperty`'s geometry without any JSON tree at all, and only a member
//! that is no instance becomes a tree. Either way there is one deliberate
//! exception: a geometry carrying a JSON-LD keyword member, such as an
//! `@context`, is not read as a typed geometry, so the member stays as written
//! for the clause 5.5.7 screening that refuses it.

use crate::attribute::{
    geo_property::GeoProperty,
    instance::{Attribute, AttributeValue},
    member_node::{MemberForm, read_member},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde::{
    Deserialize,
    Deserializer,
    de::{IgnoredAny, MapAccess, Visitor},
};
use serde_json::{Deserializer as JsonReader, Value, value::RawValue};
use std::fmt;

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
    ///
    /// The member is read by [`AttributeValue::from_node`]'s reading; a member
    /// carried verbatim keeps the spelling it was given in, and so does one
    /// whose JSON the reader would not accept.
    #[must_use]
    pub fn from_member(member: Value) -> AttributeValue {
        match read_member(&member) {
            Ok(MemberForm::Single(instance)) => AttributeValue::Single(instance),
            Ok(MemberForm::Multiple(instances)) => AttributeValue::Multiple(instances),
            Ok(MemberForm::Verbatim) | Err(_) => AttributeValue::Raw(member),
        }
    }

    /// Read one attribute member from its JSON text, as
    /// [`AttributeValue::from_member`] reads its tree.
    ///
    /// Numbers held as JSON keep the spelling of the text.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure to read a member that is no instance as
    /// the JSON value it is carried as, which the text of one JSON value never
    /// draws.
    pub(crate) fn from_member_text(member: &RawValue) -> Result<AttributeValue, serde_json::Error> {
        match member.get().trim_start().as_bytes().first() {
            Some(b'{') => match attribute_of_text(member) {
                Ok(instance) => Ok(AttributeValue::Single(Box::new(instance))),
                Err(_) => verbatim(member),
            },
            Some(b'[') => match instances_of_text(member) {
                Ok(instances) => Ok(AttributeValue::Multiple(instances)),
                Err(_) => verbatim(member),
            },
            Some(_) | None => verbatim(member),
        }
    }
}

/// Decode one attribute instance from its text, as [`AttributeValue::from_node`]
/// reads its tree: a `GeoProperty` by its geometry-aware reader, every other
/// family as the derived decoding reads it.
fn attribute_of_text(instance: &RawValue) -> Result<Attribute, serde_json::Error> {
    let mut reader = JsonReader::from_str(instance.get());
    let attribute = match serde_json::from_str::<InstanceFamily>(instance.get())? {
        InstanceFamily::GeoProperty => GeoProperty::from_instance(&mut reader).map(Attribute::GeoProperty),
        InstanceFamily::Other => Attribute::deserialize(&mut reader),
    }?;
    reader.end()?;
    Ok(attribute)
}

/// Decode every item of an array member from its text, each as one instance.
fn instances_of_text(member: &RawValue) -> Result<Vec<Attribute>, serde_json::Error> {
    serde_json::from_str::<Vec<&RawValue>>(member.get())?
        .into_iter()
        .map(attribute_of_text)
        .collect()
}

/// Carry a member that is no instance as the JSON value it is.
fn verbatim(member: &RawValue) -> Result<AttributeValue, serde_json::Error> {
    serde_json::from_str(member.get()).map(AttributeValue::Raw)
}

/// Whether an attribute instance names the `GeoProperty` family, read from
/// its `type` member alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstanceFamily {
    /// The `type` member is the string `GeoProperty`.
    GeoProperty,
    /// Anything else, a missing `type` included.
    Other,
}

// Hand-written: every member but `type` is skipped unread, and a member
// written twice counts as written last, as a parsed tree keeps it.
impl<'de> Deserialize<'de> for InstanceFamily {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<InstanceFamily, D::Error> {
        deserializer.deserialize_map(InstanceFamilyVisitor)
    }
}

/// Reads an instance's family from its `type` member.
#[derive(Clone, Copy, Debug)]
struct InstanceFamilyVisitor;

impl<'de> Visitor<'de> for InstanceFamilyVisitor {
    type Value = InstanceFamily;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an attribute instance")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<InstanceFamily, A::Error> {
        let mut family = InstanceFamily::Other;
        while let Some(name) = members.next_key::<String>()? {
            if name == "type" {
                family = if members.next_value::<Value>()?.as_str() == Some(AttributeType::GeoProperty.short_name()) {
                    InstanceFamily::GeoProperty
                } else {
                    InstanceFamily::Other
                };
            } else {
                members.next_value::<IgnoredAny>()?;
            }
        }
        Ok(family)
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::{Attribute, AttributeValue};
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json, value::RawValue};

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

    /// Decode `text` every way -- through serde from the text, from its
    /// parsed tree, and from the text kept as text as a request body is
    /// decoded -- assert all agree with the derived decoding, and answer the
    /// variant.
    fn decoded_alike(text: &str) -> &'static str {
        let from_text = hand_written(&serde_json::from_str::<AttributeValue>(text).expect("the member decodes"));
        let from_tree = hand_written(&AttributeValue::from_member(serde_json::from_str(text).expect("the member parses")));
        let raw: &RawValue = serde_json::from_str(text).expect("the member is JSON");
        let from_raw_text = hand_written(&AttributeValue::from_member_text(raw).expect("the member is read"));
        let oracle = derived(&serde_json::from_str::<DerivedAttributeValue>(text).expect("the derived decoding reads the member"));
        assert_eq!(from_text, oracle, "{text}");
        assert_eq!(from_tree, oracle, "{text}");
        assert_eq!(from_raw_text, oracle, "{text}");
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
