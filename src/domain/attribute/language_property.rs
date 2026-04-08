use crate::domain::{attribute::AttributeValue, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use langtag::LangTagBuf;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_with::skip_serializing_none;
use std::fmt;

/// Key in an NGSI-LD language map: either a BCP 47 tag or the `@none` default.
///
/// Per NGSI-LD clause 5.2.32, the special key `@none` represents the
/// default/untagged value. All other keys must be valid BCP 47 language tags.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LanguageMapKey {
    /// The `@none` key for default/untagged values (NGSI-LD clause 5.2.32).
    None,
    /// A validated BCP 47 language tag.
    Tag(LangTagBuf),
}

impl fmt::Display for LanguageMapKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LanguageMapKey::None => f.write_str("@none"),
            LanguageMapKey::Tag(tag) => write!(f, "{tag}"),
        }
    }
}

impl Serialize for LanguageMapKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            LanguageMapKey::None => serializer.serialize_str("@none"),
            LanguageMapKey::Tag(tag) => serializer.serialize_str(tag.as_str()),
        }
    }
}

impl<'de> Deserialize<'de> for LanguageMapKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<LanguageMapKey, D::Error> {
        let raw = String::deserialize(deserializer)?;
        if raw == "@none" {
            return Ok(LanguageMapKey::None);
        }
        LangTagBuf::new(raw)
            .map(LanguageMapKey::Tag)
            .map_err(|error| D::Error::custom(format!("invalid BCP 47 language tag: {error}")))
    }
}

/// Value in an NGSI-LD language map (clause 5.7.2.5).
///
/// A language map entry can be either a single string or an array of strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LanguageMapValue {
    /// A single string value.
    Single(String),
    /// An array of string values.
    Multiple(Vec<String>),
}

/// NGSI-LD LanguageProperty representation (ETSI GS CIM 009 clause 5.2.32).
///
/// A LanguageProperty represents an attribute whose value is a language map
/// (BCP47 language tag to string mapping). Contains the language map,
/// optional metadata (observedAt, datasetId), and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageProperty {
    pub language_map: IndexMap<LanguageMapKey, LanguageMapValue>,
    #[serde(default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_map_key_serializes_none() {
        let key = LanguageMapKey::None;
        let json = serde_json::to_value(&key).unwrap();
        assert_eq!(json, serde_json::Value::String("@none".to_string()));
    }

    #[test]
    fn language_map_key_serializes_tag() {
        let key = LanguageMapKey::Tag(LangTagBuf::new(String::from("en")).unwrap());
        let json = serde_json::to_value(&key).unwrap();
        assert_eq!(json, serde_json::Value::String("en".to_string()));
    }

    #[test]
    fn language_map_key_deserializes_none() {
        let key: LanguageMapKey = serde_json::from_value(serde_json::json!("@none")).unwrap();
        assert_eq!(key, LanguageMapKey::None);
    }

    #[test]
    fn language_map_key_deserializes_tag() {
        let key: LanguageMapKey = serde_json::from_value(serde_json::json!("fr-CA")).unwrap();
        let LanguageMapKey::Tag(tag) = &key else {
            panic!("expected Tag variant");
        };
        assert_eq!(tag.as_str(), "fr-CA");
    }

    #[test]
    fn language_map_key_deserializes_invalid_tag_fails() {
        let result = serde_json::from_value::<LanguageMapKey>(serde_json::json!("not a tag!!!"));
        assert!(result.is_err());
    }

    #[test]
    fn language_map_value_serializes_single() {
        let value = LanguageMapValue::Single("hello".into());
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, serde_json::json!("hello"));
    }

    #[test]
    fn language_map_value_serializes_multiple() {
        let value = LanguageMapValue::Multiple(vec!["a".into(), "b".into()]);
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn language_map_value_deserializes_single() {
        let value: LanguageMapValue = serde_json::from_value(serde_json::json!("hello")).unwrap();
        let LanguageMapValue::Single(s) = &value else {
            panic!("expected Single variant");
        };
        assert_eq!(s, "hello");
    }

    #[test]
    fn language_map_value_deserializes_multiple() {
        let value: LanguageMapValue = serde_json::from_value(serde_json::json!(["a", "b"])).unwrap();
        let LanguageMapValue::Multiple(items) = &value else {
            panic!("expected Multiple variant");
        };
        assert_eq!(items, &["a", "b"]);
    }

    #[test]
    fn language_property_round_trip() {
        let json = serde_json::json!({
            "languageMap": {
                "@none": "default",
                "en": "hello",
                "de": ["hallo", "guten tag"]
            }
        });
        let prop: LanguageProperty = serde_json::from_value(json).unwrap();
        assert_eq!(prop.language_map.len(), 3);

        let none_value = &prop.language_map[&LanguageMapKey::None];
        let LanguageMapValue::Single(s) = none_value else {
            panic!("expected Single for @none");
        };
        assert_eq!(s, "default");

        let en_key = LanguageMapKey::Tag(LangTagBuf::new(String::from("en")).unwrap());
        let en_value = &prop.language_map[&en_key];
        let LanguageMapValue::Single(s) = en_value else {
            panic!("expected Single for en");
        };
        assert_eq!(s, "hello");

        let de_key = LanguageMapKey::Tag(LangTagBuf::new(String::from("de")).unwrap());
        let de_value = &prop.language_map[&de_key];
        let LanguageMapValue::Multiple(items) = de_value else {
            panic!("expected Multiple for de");
        };
        assert_eq!(items, &["hallo", "guten tag"]);

        let reserialized = serde_json::to_value(&prop).unwrap();
        assert_eq!(reserialized["languageMap"]["@none"], "default");
        assert_eq!(reserialized["languageMap"]["en"], "hello");
        assert_eq!(reserialized["languageMap"]["de"], serde_json::json!(["hallo", "guten tag"]));
    }

    #[test]
    fn language_property_round_trip_with_none_key() {
        let json = serde_json::json!({
            "languageMap": {
                "@none": "default",
                "en": "hello"
            }
        });
        let prop: LanguageProperty = serde_json::from_value(json.clone()).unwrap();
        let reserialized = serde_json::to_value(&prop).unwrap();
        assert_eq!(reserialized["languageMap"]["@none"], "default");
        assert_eq!(reserialized["languageMap"]["en"], "hello");
    }
}
