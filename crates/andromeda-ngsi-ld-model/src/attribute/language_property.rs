//! The `LanguageProperty` attribute family and its language map (ETSI GS CIM 009 clause 5.2.32).

use crate::attribute::common_fields::CommonAttributeFields;
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use derive_more::{Deref, DerefMut, From};
use indexmap::IndexMap;
use langtag::{InvalidLangTag, LangTagBuf};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError};
use serde_with::skip_serializing_none;
use std::fmt;
use thiserror::Error;

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
            .map_err(|error| DeserializeError::custom(InvalidLanguageMapKey(error)))
    }
}

/// A language-map key that is neither `@none` nor a BCP 47 language tag.
#[derive(Debug, Error)]
#[error("invalid BCP47 language tag: {0}")]
struct InvalidLanguageMapKey(#[source] InvalidLangTag<String>);

/// Value in an NGSI-LD language map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LanguageMapValue {
    /// A single localized string.
    Single(String),
    /// Multiple localized strings for the same tag.
    Multiple(Vec<String>),
}

/// The language-tagged strings of a `LanguageProperty`, keyed by tag and
/// compared by content rather than insertion order.
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

/// NGSI-LD `LanguageProperty` representation (ETSI GS CIM 009 clause 5.2.32).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageProperty {
    /// Language-indexed string map.
    pub language_map: LanguageMap,
    /// Fixed JSON-LD datatype for language-tagged strings.
    pub value_type: Option<NgsiLdReference>,
    /// Previous language map emitted in notifications when `showChanges` is active.
    pub previous_language_map: Option<LanguageMap>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, language_property::LanguageProperty};
    use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;

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
    fn the_language_property_default_matches_the_field_by_field_literal() {
        let actual = LanguageProperty::default();
        assert!(actual.language_map.is_empty());
        assert!(actual.value_type.is_none() && actual.previous_language_map.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
