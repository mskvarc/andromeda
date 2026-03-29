use crate::{
    attribute::{
        geo_property::GeoProperty,
        json_property::JsonProperty,
        language_property::LanguageProperty,
        list_property::ListProperty,
        list_relationship::ListRelationship,
        property::Property,
        relationship::Relationship,
        vocab_property::VocabProperty,
    },
    error::{Error, Result},
    name::NameBuf,
    representation::{NgsiLdRepresentation, NgsiLdSkipNull, ToRepresentation},
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub mod geo_property;
pub mod json_property;
pub mod language_property;
pub mod list_property;
pub mod list_relationship;
pub mod property;
pub mod relationship;
pub mod vocab_property;

/// A single attribute or a multi-instance array of attributes.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum AttributeWrapper {
    Single(Attribute),
    Multi(Vec<Attribute>),
}

impl<'de> Deserialize<'de> for AttributeWrapper {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = JsonValue::deserialize(deserializer)?;
        match &v {
            JsonValue::Array(arr) => {
                let attrs = arr
                    .iter()
                    .map(Attribute::from_json_value)
                    .collect::<Result<Vec<_>>>()
                    .map_err(serde::de::Error::custom)?;
                Ok(AttributeWrapper::Multi(attrs))
            }
            _ => Ok(AttributeWrapper::Single(Attribute::from_json_value(&v).map_err(serde::de::Error::custom)?)),
        }
    }
}

impl ToRepresentation for AttributeWrapper {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match self {
            AttributeWrapper::Single(attr) => attr.to_representation(representation, skip_null),
            AttributeWrapper::Multi(attrs) => {
                let mut vals = Vec::new();
                for attr in attrs {
                    if let Some(v) = attr.to_representation(representation, skip_null)? {
                        vals.push(v);
                    }
                }
                if vals.is_empty() { Ok(None) } else { Ok(Some(JsonValue::Array(vals))) }
            }
        }
    }
}

/// The eight NGSI-LD attribute types (ETSI GS CIM 009 clause 4.5).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum Attribute {
    Property(Property),
    Relationship(Relationship),
    GeoProperty(GeoProperty),
    ListRelationship(ListRelationship),
    LanguageProperty(LanguageProperty),
    VocabProperty(VocabProperty),
    ListProperty(ListProperty),
    JsonProperty(JsonProperty),
}

/// GeoJSON geometry type names for type detection.
const GEOJSON_TYPES: &[&str] = &[
    "Point",
    "MultiPoint",
    "LineString",
    "MultiLineString",
    "Polygon",
    "MultiPolygon",
    "GeometryCollection",
];

impl Attribute {
    /// Deserialize from a `serde_json::Value`, auto-detecting the NGSI-LD
    /// representation format (normalized, concise, or simplified).
    pub fn from_json_value(v: &JsonValue) -> Result<Self> {
        let obj = match v {
            // Scalar → simplified Property
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {
                let val = ngsi_ld_value::Value::from(v.clone());
                return Ok(Attribute::Property(Property::new(val)));
            }
            JsonValue::Object(m) => m,
            // Array at this level is unexpected (handled by AttributeWrapper)
            JsonValue::Array(_) => {
                return Err(Error::Validation("Unexpected array in attribute position".to_string()));
            }
        };

        // Check for "type" key to determine format
        if let Some(type_str) = obj.get("type").and_then(|t| t.as_str()) {
            match type_str {
                // NGSI-LD normalized types
                "Property" => {
                    return serde_json::from_value(v.clone()).map(Attribute::Property).map_err(Error::from);
                }
                "Relationship" => {
                    return serde_json::from_value(v.clone()).map(Attribute::Relationship).map_err(Error::from);
                }
                "GeoProperty" => {
                    return serde_json::from_value(v.clone()).map(Attribute::GeoProperty).map_err(Error::from);
                }
                "LanguageProperty" => {
                    return serde_json::from_value(v.clone()).map(Attribute::LanguageProperty).map_err(Error::from);
                }
                "JsonProperty" => {
                    return serde_json::from_value(v.clone()).map(Attribute::JsonProperty).map_err(Error::from);
                }
                "VocabProperty" => {
                    return serde_json::from_value(v.clone()).map(Attribute::VocabProperty).map_err(Error::from);
                }
                "ListProperty" => {
                    return serde_json::from_value(v.clone()).map(Attribute::ListProperty).map_err(Error::from);
                }
                "ListRelationship" => {
                    return serde_json::from_value(v.clone()).map(Attribute::ListRelationship).map_err(Error::from);
                }
                // GeoJSON type → concise GeoProperty (the object IS the geometry)
                t if GEOJSON_TYPES.contains(&t) => {
                    let val = ngsi_ld_value::Value::from(v.clone());
                    return Ok(Attribute::GeoProperty(GeoProperty::new(val)));
                }
                _ => {}
            }
        }

        // No NGSI-LD type tag → concise format; detect by discriminating keys
        if obj.contains_key("value") {
            return serde_json::from_value(v.clone()).map(Attribute::Property).map_err(Error::from);
        }
        if obj.contains_key("object") {
            return serde_json::from_value(v.clone()).map(Attribute::Relationship).map_err(Error::from);
        }
        if obj.contains_key("languageMap") {
            return serde_json::from_value(v.clone()).map(Attribute::LanguageProperty).map_err(Error::from);
        }
        if obj.contains_key("hasVocab") {
            return serde_json::from_value(v.clone()).map(Attribute::VocabProperty).map_err(Error::from);
        }
        if obj.contains_key("hasJson") {
            return serde_json::from_value(v.clone()).map(Attribute::JsonProperty).map_err(Error::from);
        }
        if obj.contains_key("hasValueList") {
            return serde_json::from_value(v.clone()).map(Attribute::ListProperty).map_err(Error::from);
        }
        if obj.contains_key("objectList") {
            return serde_json::from_value(v.clone()).map(Attribute::ListRelationship).map_err(Error::from);
        }

        // Unknown object → treat as Property with object value
        let val = ngsi_ld_value::Value::from(v.clone());
        Ok(Attribute::Property(Property::new(val)))
    }
}

impl ToRepresentation for Attribute {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        if skip_null == NgsiLdSkipNull::Skip {
            match self {
                Attribute::Property(p) => {
                    if p.value.is_null() || p.value.to_string().is_empty() {
                        return Ok(None);
                    }
                }
                Attribute::GeoProperty(g) => {
                    if g.value.is_null() || g.value.to_string().is_empty() {
                        return Ok(None);
                    }
                }
                Attribute::LanguageProperty(lp) => {
                    if lp.language_map.is_empty() {
                        return Ok(None);
                    }
                }
                Attribute::VocabProperty(vp) => {
                    if vp.has_vocab.is_empty() {
                        return Ok(None);
                    }
                }
                Attribute::ListProperty(lp) => {
                    if lp.has_value_list.is_empty() {
                        return Ok(None);
                    }
                }
                Attribute::JsonProperty(jp) => {
                    if jp.has_json.is_null() || jp.has_json.to_string().is_empty() {
                        return Ok(None);
                    }
                }
                _ => {}
            }
        }

        match self {
            Attribute::Property(p) => p.to_representation(representation, skip_null),
            Attribute::Relationship(r) => r.to_representation(representation, skip_null),
            Attribute::GeoProperty(g) => g.to_representation(representation, skip_null),
            Attribute::ListRelationship(lr) => lr.to_representation(representation, skip_null),
            Attribute::LanguageProperty(lp) => lp.to_representation(representation, skip_null),
            Attribute::VocabProperty(vp) => vp.to_representation(representation, skip_null),
            Attribute::ListProperty(lp) => lp.to_representation(representation, skip_null),
            Attribute::JsonProperty(jp) => jp.to_representation(representation, skip_null),
        }
    }
}

/// Ordered map of attribute names to attribute wrappers.
pub type Attributes = IndexMap<NameBuf, AttributeWrapper>;
