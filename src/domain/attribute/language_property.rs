use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::{JsonLdExpandableMacro, JsonLdLanguageMap},
};
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

impl JsonLdLanguageMap for IndexMap<LanguageMapKey, LanguageMapValue> {
    fn to_expanded_language_map(&self) -> serde_json::Value {
        let mut entries = Vec::new();
        for (key, value) in self {
            let texts: Vec<&str> = match value {
                LanguageMapValue::Single(s) => vec![s.as_str()],
                LanguageMapValue::Multiple(v) => v.iter().map(String::as_str).collect(),
            };
            for text in texts {
                let mut entry = serde_json::Map::new();
                entry.insert("@value".to_string(), serde_json::Value::String(text.to_string()));
                match key {
                    LanguageMapKey::None => {}
                    LanguageMapKey::Tag(tag) => {
                        entry.insert("@language".to_string(), serde_json::Value::String(tag.as_str().to_string()));
                    }
                }
                entries.push(serde_json::Value::Object(entry));
            }
        }
        serde_json::Value::Array(entries)
    }
}

/// NGSI-LD LanguageProperty representation (ETSI GS CIM 009 clause 5.2.32).
///
/// A LanguageProperty represents an attribute whose value is a language map
/// (BCP47 language tag to string mapping). Contains the language map,
/// optional metadata (observedAt, datasetId), and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/LanguageProperty")]
pub struct LanguageProperty {
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/hasLanguageMap", language_map)]
    pub language_map: IndexMap<LanguageMapKey, LanguageMapValue>,
    #[serde(default)]
    #[jsonld(
        property = "https://uri.etsi.org/ngsi-ld/observedAt",
        typed_value,
        datatype = "https://uri.etsi.org/ngsi-ld/DateTime"
    )]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[jsonld(
        property = "https://uri.etsi.org/ngsi-ld/expiresAt",
        typed_value,
        datatype = "https://uri.etsi.org/ngsi-ld/DateTime"
    )]
    pub expires_at: Option<DateTime<Utc>>,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/datasetId", id_ref)]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
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

    #[test]
    fn language_property_expanded_produces_language_map() {
        use crate::jsonld::JsonLdExpandable;

        let json = serde_json::json!({
            "type": "LanguageProperty",
            "languageMap": {
                "@none": "default",
                "en": "hello",
                "de": ["hallo", "guten tag"]
            }
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("language_property");
        let crate::domain::attribute::Attribute::LanguageProperty(lp) = &attr else {
            panic!("expected LanguageProperty");
        };
        let expanded = lp.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/LanguageProperty"]));

        let lang_map = obj["https://uri.etsi.org/ngsi-ld/hasLanguageMap"].as_array().expect("language map array");
        // @none entry has no @language
        assert!(lang_map.iter().any(|e| e["@value"] == "default" && e.get("@language").is_none()));
        // en entry
        assert!(lang_map.iter().any(|e| e["@value"] == "hello" && e["@language"] == "en"));
        // de entries (multiple)
        assert!(lang_map.iter().any(|e| e["@value"] == "hallo" && e["@language"] == "de"));
        assert!(lang_map.iter().any(|e| e["@value"] == "guten tag" && e["@language"] == "de"));
    }
}
