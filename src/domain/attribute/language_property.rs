use crate::{
    domain::attribute::{AttributeValue, common::CommonAttributeFields},
    jsonld::{JsonLdExpandableMacro, JsonLdLanguageMap},
};
use compact_str::CompactString;
use indexmap::IndexMap;
use langtag::LangTagBuf;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_with::skip_serializing_none;
use std::fmt;

/// Key in an NGSI-LD language map: either a BCP 47 tag or `@none`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LanguageMapKey {
    /// Default untagged language entry.
    None,
    /// Valid BCP 47 language tag.
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
        serializer.serialize_str(&self.to_string())
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
            .map_err(|error| D::Error::custom(format!("invalid BCP47 language tag: {error}")))
    }
}

/// Value in an NGSI-LD language map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LanguageMapValue {
    /// A single localized string.
    Single(String),
    /// Multiple localized strings for the same tag.
    Multiple(Vec<String>),
}

impl JsonLdLanguageMap for IndexMap<LanguageMapKey, LanguageMapValue> {
    fn to_expanded_language_map(&self) -> serde_json::Value {
        let entries = self
            .iter()
            .flat_map(|(key, value)| match value {
                LanguageMapValue::Single(text) => vec![language_entry(key, text)],
                LanguageMapValue::Multiple(values) => values.iter().map(|text| language_entry(key, text)).collect(),
            })
            .collect();
        serde_json::Value::Array(entries)
    }
}

/// NGSI-LD LanguageProperty representation (ETSI GS CIM 009 clause 5.2.32).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:LanguageProperty")]
pub struct LanguageProperty {
    /// Language-indexed string map.
    #[jsonld(property = "ngsi:hasLanguageMap", language_map)]
    pub language_map: IndexMap<LanguageMapKey, LanguageMapValue>,
    /// Fixed JSON-LD datatype for language-tagged strings.
    #[jsonld(property = "ngsi:hasValueType", vocab)]
    pub value_type: Option<CompactString>,
    /// Previous language map emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousLanguageMap", language_map)]
    pub previous_language_map: Option<IndexMap<LanguageMapKey, LanguageMapValue>>,
    #[serde(flatten)]
    #[jsonld(flatten_object)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

impl Default for LanguageProperty {
    fn default() -> LanguageProperty {
        LanguageProperty {
            language_map: IndexMap::new(),
            value_type: None,
            previous_language_map: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

fn language_entry(key: &LanguageMapKey, value: &str) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    entry.insert("@value".to_string(), serde_json::Value::String(value.to_string()));
    if let LanguageMapKey::Tag(tag) = key {
        entry.insert("@language".to_string(), serde_json::Value::String(tag.as_str().to_string()));
    }
    serde_json::Value::Object(entry)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::{
        domain::attribute::{Attribute, LanguageMapKey, LanguageMapValue},
        jsonld::{JsonLdExpandable, vocab},
    };
    use indexmap::IndexMap;

    #[test]
    fn language_property_round_trip_supports_previous_language_map() {
        let json = serde_json::json!({
            "type": "LanguageProperty",
            "languageMap": { "en": "hello" },
            "previousLanguageMap": { "en": "hi" },
            "valueType": "langString"
        });
        let attr: Attribute = serde_json::from_value(json).expect("language property");
        let Attribute::LanguageProperty(property) = attr else {
            unreachable!("expected LanguageProperty");
        };
        assert!(property.previous_language_map.is_some());
        pretty_assertions::assert_eq!(property.value_type.as_deref(), Some("langString"));
    }

    #[test]
    fn language_map_expands_as_language_tagged_values() {
        let mut map = IndexMap::new();
        map.insert(LanguageMapKey::None, LanguageMapValue::Single("default".to_string()));
        let property = crate::domain::attribute::LanguageProperty {
            language_map: map,
            ..Default::default()
        };
        let expanded = property.to_expanded();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::expanded::LANGUAGE_MAP.as_str()));
    }
}
