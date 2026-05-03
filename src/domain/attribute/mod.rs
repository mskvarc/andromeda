pub mod common;
mod geo_property;
mod json_property;
mod language_property;
mod list_property;
mod list_relationship;
mod property;
mod relationship;
mod vocab_property;

pub use geo_property::GeoProperty;
pub use json_property::JsonProperty;
pub use language_property::{LanguageMapKey, LanguageMapValue, LanguageProperty};
pub use list_property::ListProperty;
pub use list_relationship::ListRelationship;
pub use property::Property;
pub use relationship::Relationship;
pub use vocab_property::VocabProperty;

use crate::jsonld::Expandable;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Attribute {
    Property(Property),
    Relationship(Relationship),
    VocabProperty(VocabProperty),
    GeoProperty(GeoProperty),
    LanguageProperty(LanguageProperty),
    JsonProperty(JsonProperty),
    ListProperty(ListProperty),
    ListRelationship(ListRelationship),
}

impl Attribute {
    pub fn value(&self) -> serde_json::Value {
        match self {
            Attribute::Property(p) => serde_json::to_value(&p.value).unwrap_or_default(),
            Attribute::Relationship(r) => serde_json::to_value(&r.object).unwrap_or_default(),
            Attribute::VocabProperty(v) => serde_json::to_value(&v.vocab).unwrap_or_default(),
            Attribute::GeoProperty(g) => serde_json::json!(g.value),
            // Serialization of IndexMap<LanguageMapKey, LanguageMapValue> to JSON cannot fail
            // (keys serialize to strings, values to strings or arrays of strings).
            Attribute::LanguageProperty(l) => serde_json::to_value(&l.language_map).unwrap_or_default(),
            Attribute::JsonProperty(j) => j.json.clone(),
            Attribute::ListProperty(lp) => serde_json::to_value(&lp.value_list).unwrap_or_default(),
            Attribute::ListRelationship(lr) => serde_json::to_value(&lr.object_list).unwrap_or_default(),
        }
    }

    /// Returns the `expiresAt` value of the underlying attribute instance, if set.
    pub const fn expires_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Attribute::Property(p) => p.common.expires_at,
            Attribute::Relationship(r) => r.common.expires_at,
            Attribute::VocabProperty(v) => v.common.expires_at,
            Attribute::GeoProperty(g) => g.common.expires_at,
            Attribute::LanguageProperty(l) => l.common.expires_at,
            Attribute::JsonProperty(j) => j.common.expires_at,
            Attribute::ListProperty(lp) => lp.common.expires_at,
            Attribute::ListRelationship(lr) => lr.common.expires_at,
        }
    }

    pub fn dataset_id_string(&self) -> Option<String> {
        let uri = match self {
            Attribute::Property(p) => p.common.dataset_id.as_ref(),
            Attribute::Relationship(r) => r.common.dataset_id.as_ref(),
            Attribute::VocabProperty(v) => v.common.dataset_id.as_ref(),
            Attribute::GeoProperty(g) => g.common.dataset_id.as_ref(),
            Attribute::LanguageProperty(l) => l.common.dataset_id.as_ref(),
            Attribute::JsonProperty(j) => j.common.dataset_id.as_ref(),
            Attribute::ListProperty(lp) => lp.common.dataset_id.as_ref(),
            Attribute::ListRelationship(lr) => lr.common.dataset_id.as_ref(),
        };
        uri.map(|u| u.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    Single(Box<Attribute>),
    Multiple(Vec<Attribute>),
    Raw(serde_json::Value),
}

impl Expandable for Attribute {
    fn expand<V: jsonld_expandable_core::JsonValue>(&self) -> V {
        match self {
            Attribute::Property(p) => Expandable::expand::<V>(p),
            Attribute::Relationship(r) => Expandable::expand::<V>(r),
            Attribute::VocabProperty(v) => Expandable::expand::<V>(v),
            Attribute::GeoProperty(g) => Expandable::expand::<V>(g),
            Attribute::LanguageProperty(l) => Expandable::expand::<V>(l),
            Attribute::JsonProperty(j) => Expandable::expand::<V>(j),
            Attribute::ListProperty(lp) => Expandable::expand::<V>(lp),
            Attribute::ListRelationship(lr) => Expandable::expand::<V>(lr),
        }
    }
}

impl Expandable for AttributeValue {
    fn expand<V: jsonld_expandable_core::JsonValue>(&self) -> V {
        match self {
            AttributeValue::Single(attr) => V::array(std::iter::once(Expandable::expand::<V>(attr.as_ref()))),
            AttributeValue::Multiple(attrs) => V::array(attrs.iter().map(Expandable::expand::<V>)),
            AttributeValue::Raw(value) => V::array(std::iter::once(V::object(std::iter::once((
                "@value".to_string(),
                serde_json_value_to_json_value::<V>(value),
            ))))),
        }
    }
}

impl<V: jsonld_expandable_core::JsonValue> jsonld_expandable_core::ToJsonValue<V> for AttributeValue {
    fn to_json_value(&self) -> V {
        Expandable::expand::<V>(self)
    }
}

pub(crate) fn serde_json_value_to_json_value<V: jsonld_expandable_core::JsonValue>(value: &serde_json::Value) -> V {
    match value {
        serde_json::Value::Null => V::null(),
        serde_json::Value::Bool(b) => V::bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                V::integer(i)
            } else if let Some(u) = n.as_u64() {
                V::unsigned(u)
            } else if let Some(f) = n.as_f64() {
                V::float(f)
            } else {
                V::null()
            }
        }
        serde_json::Value::String(s) => V::string(s),
        serde_json::Value::Array(items) => V::array(items.iter().map(serde_json_value_to_json_value::<V>)),
        serde_json::Value::Object(map) => V::object(map.iter().map(|(k, v)| (k.clone(), serde_json_value_to_json_value::<V>(v)))),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 25,
            "expiresAt": "2030-12-31T23:59:59Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::Property(ref p) = attr else {
            unreachable!("expected Property");
        };
        pretty_assertions::assert_eq!(p.common.expires_at, Some(Utc.with_ymd_and_hms(2030, 12, 31, 23, 59, 59).unwrap()));
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-12-31T23:59:59Z");
    }

    #[test]
    fn property_omits_expires_at_when_none() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 42
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_value(&attr).unwrap();
        assert!(serialized.get("expiresAt").is_none());
    }

    #[test]
    fn relationship_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1",
            "expiresAt": "2030-06-15T12:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::Relationship(ref r) = attr else {
            unreachable!("expected Relationship");
        };
        pretty_assertions::assert_eq!(r.common.expires_at, Some(Utc.with_ymd_and_hms(2030, 6, 15, 12, 0, 0).unwrap()));
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-06-15T12:00:00Z");
    }

    #[test]
    fn geo_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.0, 52.0]},
            "expiresAt": "2030-01-01T00:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::GeoProperty(ref g) = attr else {
            unreachable!("expected GeoProperty");
        };
        assert!(g.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-01-01T00:00:00Z");
    }

    #[test]
    fn vocab_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "VocabProperty",
            "vocab": "categoryA",
            "expiresAt": "2030-03-15T08:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::VocabProperty(ref v) = attr else {
            unreachable!("expected VocabProperty");
        };
        assert!(v.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-03-15T08:00:00Z");
    }

    #[test]
    fn language_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "LanguageProperty",
            "languageMap": {"en": "hello", "de": "hallo"},
            "expiresAt": "2030-07-01T00:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::LanguageProperty(ref l) = attr else {
            unreachable!("expected LanguageProperty");
        };
        assert!(l.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-07-01T00:00:00Z");
    }

    #[test]
    fn json_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": {"key": "value"},
            "expiresAt": "2030-09-01T12:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::JsonProperty(ref j) = attr else {
            unreachable!("expected JsonProperty");
        };
        assert!(j.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-09-01T12:00:00Z");
    }

    #[test]
    fn list_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "ListProperty",
            "valueList": [1, 2, 3],
            "expiresAt": "2030-11-20T06:30:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::ListProperty(ref lp) = attr else {
            unreachable!("expected ListProperty");
        };
        assert!(lp.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-11-20T06:30:00Z");
    }

    #[test]
    fn list_relationship_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"],
            "expiresAt": "2030-04-10T18:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::ListRelationship(ref lr) = attr else {
            unreachable!("expected ListRelationship");
        };
        assert!(lr.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-04-10T18:00:00Z");
    }

    #[test]
    fn attribute_enum_expires_at_returns_value() {
        let dt = Utc.with_ymd_and_hms(2030, 6, 1, 0, 0, 0).unwrap();
        let json = serde_json::json!({
            "type": "Property",
            "value": 10,
            "expiresAt": "2030-06-01T00:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        pretty_assertions::assert_eq!(attr.expires_at(), Some(dt));
    }

    #[test]
    fn attribute_enum_expires_at_returns_none() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 10
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        pretty_assertions::assert_eq!(attr.expires_at(), None);
    }

    #[test]
    fn attribute_enum_delegates_to_expanded_to_inner_type() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 42
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let expanded = attr.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("expanded is object");

        // Enum delegation should produce the same output as the inner struct
        pretty_assertions::assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Property"]));
        pretty_assertions::assert_eq!(obj["https://uri.etsi.org/ngsi-ld/hasValue"], serde_json::json!([{"@value": 42}]));
    }

    #[test]
    fn attribute_enum_delegates_relationship_expansion() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let expanded = attr.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("expanded is object");

        pretty_assertions::assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Relationship"]));
    }

    #[test]
    fn attribute_value_single_wraps_in_array() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 99
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let av = AttributeValue::Single(Box::new(attr));
        let expanded = av.expand::<serde_json::Value>();
        let arr = expanded.as_array().expect("expanded is array");
        pretty_assertions::assert_eq!(arr.len(), 1);
        pretty_assertions::assert_eq!(arr[0]["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Property"]));
    }

    #[test]
    fn attribute_value_multiple_produces_array() {
        let json1 = serde_json::json!({"type": "Property", "value": 1});
        let json2 = serde_json::json!({"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:1"});
        let a1: Attribute = serde_json::from_value(json1).unwrap();
        let a2: Attribute = serde_json::from_value(json2).unwrap();
        let av = AttributeValue::Multiple(vec![a1, a2]);
        let expanded = av.expand::<serde_json::Value>();
        let arr = expanded.as_array().expect("expanded is array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }

    #[test]
    fn attribute_value_raw_wraps_in_value_object() {
        let av = AttributeValue::Raw(serde_json::json!("hello"));
        let expanded = av.expand::<serde_json::Value>();
        pretty_assertions::assert_eq!(expanded, serde_json::json!([{"@value": "hello"}]));
    }
}
