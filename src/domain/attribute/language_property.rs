use crate::{
    domain::attribute::{AttributeValue, common::CommonAttributeFields},
    jsonld::{Expandable, ExpandableLanguageMap, JsonValue},
};
use compact_str::CompactString;
use derive_more::{Deref, From};
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

/// Local newtype around [`IndexMap`] of language-tagged strings, required so
/// the foreign [`ExpandableLanguageMap`] trait can be implemented (orphan
/// rules forbid implementing a foreign trait for the foreign [`IndexMap`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize, From, Deref)]
#[serde(transparent)]
pub struct LanguageMap(pub IndexMap<LanguageMapKey, LanguageMapValue>);

impl std::ops::DerefMut for LanguageMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl PartialEq for LanguageMap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().all(|(k, v)| other.0.get(k).is_some_and(|other_v| other_v == v))
    }
}

impl Eq for LanguageMap {}

impl ExpandableLanguageMap for LanguageMap {
    fn to_expanded_language_map<V: JsonValue>(&self) -> V {
        let entries: Vec<V> = self
            .0
            .iter()
            .flat_map(|(key, value)| match value {
                LanguageMapValue::Single(text) => vec![language_entry::<V>(key, text)],
                LanguageMapValue::Multiple(values) => values.iter().map(|text| language_entry::<V>(key, text)).collect(),
            })
            .collect();
        V::array(entries)
    }
}

/// NGSI-LD LanguageProperty representation (ETSI GS CIM 009 clause 5.2.32).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:LanguageProperty",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct LanguageProperty {
    /// Language-indexed string map.
    #[jsonld(property = "ngsi:hasLanguageMap", language_map)]
    pub language_map: LanguageMap,
    /// Fixed JSON-LD datatype for language-tagged strings.
    #[jsonld(property = "ngsi:hasValueType", vocab)]
    pub value_type: Option<CompactString>,
    /// Previous language map emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousLanguageMap", language_map)]
    pub previous_language_map: Option<LanguageMap>,
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
            language_map: LanguageMap::default(),
            value_type: None,
            previous_language_map: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

fn language_entry<V: JsonValue>(key: &LanguageMapKey, value: &str) -> V {
    let mut entries: Vec<(String, V)> = Vec::with_capacity(2);
    entries.push(("@value".to_string(), V::string(value)));
    if let LanguageMapKey::Tag(tag) = key {
        entries.push(("@language".to_string(), V::string(tag.as_str())));
    }
    V::object(entries)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::{
        domain::attribute::{Attribute, LanguageMapKey, LanguageMapValue},
        jsonld::{Expandable, vocab},
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
            language_map: map.into(),
            ..Default::default()
        };
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::expanded::LANGUAGE_MAP.as_str()));
    }
}
