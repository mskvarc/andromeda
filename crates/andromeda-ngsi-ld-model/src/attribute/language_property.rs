//! The `LanguageProperty` attribute family and its language map (ETSI GS CIM 009 clause 5.2.32).

use crate::attribute::common_fields::CommonAttributeFields;
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use derive_more::{Deref, DerefMut, From};
use indexmap::IndexMap;
use jsonld::{Expandable, ExpandableLanguageMap};
use jsonld_expandable_core::JsonValue;
use langtag::LangTagBuf;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
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

// Hand-written on both halves: the key is either the JSON-LD keyword `@none`
// or a BCP 47 tag, and no derive can express that split over a foreign
// `LangTagBuf` whose validation has to run on the way in.
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
            .map_err(|error| Error::custom(format!("invalid BCP47 language tag: {error}")))
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, From, Deref, DerefMut)]
#[serde(transparent)]
pub struct LanguageMap(pub IndexMap<LanguageMapKey, LanguageMapValue>);

// `#[derive(PartialEq)]` would forward to `IndexMap`, whose equality is
// insertion-order sensitive. Two language maps carrying the same tags with the
// same values are the same map under clause 4.5.16 however they were built, so
// the comparison is by content, and `Eq` follows it rather than the derive.
impl PartialEq for LanguageMap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().all(|(k, v)| other.0.get(k).is_some_and(|other_v| other_v == v))
    }
}

// Follows the hand-written `PartialEq` above rather than the derive, which
// would demand `IndexMap: Eq` with insertion-order semantics.
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

/// NGSI-LD `LanguageProperty` representation (ETSI GS CIM 009 clause 5.2.32).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:LanguageProperty",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct LanguageProperty {
    /// Language-indexed string map.
    #[jsonld(property = "ngsi:hasLanguageMap", container = "language")]
    pub language_map: LanguageMap,
    /// Fixed JSON-LD datatype for language-tagged strings.
    #[jsonld(property = "ngsi:hasValueType", coerce = "@id")]
    pub value_type: Option<NgsiLdReference>,
    /// Previous language map emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousLanguageMap", container = "language")]
    pub previous_language_map: Option<LanguageMap>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

/// Builds one expanded language-map entry: an `@value` node carrying an
/// `@language` tag unless the key is the `@none` default (clause 4.5.16).
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
    use crate::attribute::{
        instance::Attribute,
        language_property::{LanguageMapKey, LanguageMapValue, LanguageProperty},
    };
    use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use indexmap::IndexMap;
    use jsonld::Expandable;

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
            panic!("expected LanguageProperty");
        };
        assert!(property.previous_language_map.is_some());
        pretty_assertions::assert_eq!(property.value_type.as_ref().map(NgsiLdReference::as_str), Some("langString"));
    }

    #[test]
    fn language_map_expands_as_language_tagged_values() {
        let mut map = IndexMap::new();
        map.insert(LanguageMapKey::None, LanguageMapValue::Single("default".to_string()));
        let property = LanguageProperty {
            language_map: map.into(),
            ..Default::default()
        };
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(expanded_properties::LANGUAGE_MAP.as_str()));
    }

    #[test]
    fn the_language_property_default_matches_the_field_by_field_literal() {
        let actual = LanguageProperty::default();
        assert!(actual.language_map.is_empty());
        assert!(actual.value_type.is_none() && actual.previous_language_map.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
