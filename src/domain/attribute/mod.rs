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
pub use language_property::LanguageProperty;
pub use list_property::ListProperty;
pub use list_relationship::ListRelationship;
pub use property::Property;
pub use relationship::Relationship;
pub use vocab_property::VocabProperty;

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
            Attribute::Property(p) => p.value.clone(),
            Attribute::Relationship(r) => serde_json::Value::String(r.object.to_string()),
            Attribute::VocabProperty(v) => v.vocab.clone(),
            Attribute::GeoProperty(g) => serde_json::json!(g.value),
            Attribute::LanguageProperty(l) => l.language_map.clone(),
            Attribute::JsonProperty(j) => j.json.clone(),
            Attribute::ListProperty(lp) => serde_json::json!(lp.value_list),
            Attribute::ListRelationship(lr) => serde_json::json!(lr.object_list.iter().map(|u| u.to_string()).collect::<Vec<_>>()),
        }
    }

    /// Returns the `expiresAt` value of the underlying attribute instance, if set.
    pub const fn expires_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Attribute::Property(p) => p.expires_at,
            Attribute::Relationship(r) => r.expires_at,
            Attribute::VocabProperty(v) => v.expires_at,
            Attribute::GeoProperty(g) => g.expires_at,
            Attribute::LanguageProperty(l) => l.expires_at,
            Attribute::JsonProperty(j) => j.expires_at,
            Attribute::ListProperty(lp) => lp.expires_at,
            Attribute::ListRelationship(lr) => lr.expires_at,
        }
    }

    pub fn dataset_id_string(&self) -> Option<String> {
        let uri = match self {
            Attribute::Property(p) => p.dataset_id.as_ref(),
            Attribute::Relationship(r) => r.dataset_id.as_ref(),
            Attribute::VocabProperty(v) => v.dataset_id.as_ref(),
            Attribute::GeoProperty(g) => g.dataset_id.as_ref(),
            Attribute::LanguageProperty(l) => l.dataset_id.as_ref(),
            Attribute::JsonProperty(j) => j.dataset_id.as_ref(),
            Attribute::ListProperty(lp) => lp.dataset_id.as_ref(),
            Attribute::ListRelationship(lr) => lr.dataset_id.as_ref(),
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

#[cfg(test)]
mod tests {
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
            panic!("expected Property");
        };
        assert_eq!(p.expires_at, Some(Utc.with_ymd_and_hms(2030, 12, 31, 23, 59, 59).unwrap()));
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-12-31T23:59:59Z");
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
            panic!("expected Relationship");
        };
        assert_eq!(r.expires_at, Some(Utc.with_ymd_and_hms(2030, 6, 15, 12, 0, 0).unwrap()));
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-06-15T12:00:00Z");
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
            panic!("expected GeoProperty");
        };
        assert!(g.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-01-01T00:00:00Z");
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
            panic!("expected VocabProperty");
        };
        assert!(v.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-03-15T08:00:00Z");
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
            panic!("expected LanguageProperty");
        };
        assert!(l.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-07-01T00:00:00Z");
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
            panic!("expected JsonProperty");
        };
        assert!(j.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-09-01T12:00:00Z");
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
            panic!("expected ListProperty");
        };
        assert!(lp.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-11-20T06:30:00Z");
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
            panic!("expected ListRelationship");
        };
        assert!(lr.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        assert_eq!(reserialized["expiresAt"], "2030-04-10T18:00:00Z");
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
        assert_eq!(attr.expires_at(), Some(dt));
    }

    #[test]
    fn attribute_enum_expires_at_returns_none() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 10
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        assert_eq!(attr.expires_at(), None);
    }
}
