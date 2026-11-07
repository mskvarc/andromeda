//! The `LanguageProperty` attribute family and its language map (ETSI GS CIM 009 clause 5.2.32).

use crate::attribute::{common_fields::CommonAttributeFields, one_or_many::OneOrMany};
use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;
use andromeda_ngsi_ld_representation::value_budget::ValueBudget;
use andromeda_ngsi_ld_vocabulary::keyword;
use derive_more::{Deref, DerefMut, From};
use indexmap::IndexMap;
use langtag::{InvalidLangTag, LangTagBuf};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{DeserializeSeed, Error as DeserializeError, Expected, IgnoredAny, MapAccess, Visitor},
};
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
            LanguageMapKey::None => f.write_str(keyword::NONE),
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
        if raw == keyword::NONE {
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

/// Value in an NGSI-LD language map: a string or an array of strings
/// (clause 4.5.18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, from = "OneOrMany<String>")]
pub enum LanguageMapValue {
    /// A single localized string.
    Single(String),
    /// Multiple localized strings for the same tag.
    Multiple(Vec<String>),
}

impl LanguageMapValue {
    /// The strings the value holds; each expands to one language entry.
    #[must_use]
    pub const fn string_count(&self) -> usize {
        match self {
            LanguageMapValue::Single(_) => 1,
            LanguageMapValue::Multiple(texts) => texts.len(),
        }
    }
}

// Hand-written: the two enums carry the same payloads under different variant
// names, and no derive maps one enum onto another.
impl From<OneOrMany<String>> for LanguageMapValue {
    fn from(written: OneOrMany<String>) -> LanguageMapValue {
        match written {
            OneOrMany::One(text) => LanguageMapValue::Single(text),
            OneOrMany::Many(texts) => LanguageMapValue::Multiple(texts),
        }
    }
}

/// The language-tagged strings of a `LanguageProperty`, keyed by tag and
/// compared by content rather than insertion order.
#[derive(Debug, Clone, Default, Serialize, From, Deref, DerefMut)]
#[serde(transparent)]
pub struct LanguageMap(pub IndexMap<LanguageMapKey, LanguageMapValue>);

impl LanguageMap {
    /// The strings the map holds over every tag; each expands to one language
    /// entry.
    #[must_use]
    pub fn string_count(&self) -> usize {
        self.0.values().map(LanguageMapValue::string_count).sum()
    }
}

// Hand-written: a refused entry names its language tag, which a derived map
// reading never passes to the value it reads.
impl<'de> Deserialize<'de> for LanguageMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<LanguageMap, D::Error> {
        LanguageMap::within(None).deserialize(deserializer)
    }
}

impl LanguageMap {
    /// Read a language map, its strings recorded against `budget` when there is
    /// one.
    #[must_use]
    pub const fn within(budget: Option<&ValueBudget>) -> LanguageMapWithin<'_> {
        LanguageMapWithin(budget)
    }
}

/// Reads a language map entry by entry, each string recorded against the
/// budget of values the entity it belongs to may still hold. Once the budget is
/// spent the entries that follow are skipped, so a map of millions of tags costs
/// what the budget costs whatever its size.
#[derive(Debug, Clone, Copy)]
pub struct LanguageMapWithin<'b>(Option<&'b ValueBudget>);

impl<'de> DeserializeSeed<'de> for LanguageMapWithin<'_> {
    type Value = LanguageMap;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<LanguageMap, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for LanguageMapWithin<'_> {
    type Value = LanguageMap;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a language map, an object keyed by language tag")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<LanguageMap, A::Error> {
        let mut map = IndexMap::new();
        while self.0.is_none_or(|budget| !budget.is_spent()) {
            let Some(key) = entries.next_key::<LanguageMapKey>()? else {
                return Ok(LanguageMap(map));
            };
            let value = entries.next_value_seed(LanguageMapEntry { key: &key, budget: self.0 })?;
            // A tag written twice keeps its last strings, and the count makes
            // only those.
            if let (Some(budget), Some(replaced)) = (self.0, map.insert(key, value)) {
                budget.refund(replaced.string_count());
            }
        }
        while entries.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(LanguageMap(map))
    }
}

/// The value of the language map entry under one key, refused with the key it
/// stands under.
struct LanguageMapEntry<'k, 'b> {
    /// The key the entry stands under.
    key: &'k LanguageMapKey,
    /// What the strings of the entry are recorded against.
    budget: Option<&'b ValueBudget>,
}

impl<'de> DeserializeSeed<'de> for LanguageMapEntry<'_, '_> {
    type Value = LanguageMapValue;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<LanguageMapValue, D::Error> {
        let budget = self.budget;
        OneOrMany::deserialize_expecting_within(deserializer, self, budget).map(LanguageMapValue::from)
    }
}

// `Expected` has no derive: it is the text a refusal of the entry names.
impl Expected for LanguageMapEntry<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a string or an array of strings under the language tag '{}'", self.key)
    }
}

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
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageProperty {
    /// Language-indexed string map.
    pub language_map: LanguageMap,
    /// Fixed JSON-LD datatype for language-tagged strings.
    pub value_type: Option<CoercedTerm>,
    /// Previous language map emitted in notifications when `showChanges` is active.
    pub previous_language_map: Option<LanguageMap>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{
        instance::Attribute,
        language_property::{LanguageMap, LanguageMapKey, LanguageMapValue, LanguageProperty},
    };
    use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;

    #[test]
    fn a_language_map_counts_one_string_per_expanded_entry_over_every_tag() {
        let map: LanguageMap = serde_json::from_str(r#"{"en":"a","fr":["b","c","d"],"@none":["e"]}"#).expect("a language map");
        pretty_assertions::assert_eq!(map.string_count(), 5);
        pretty_assertions::assert_eq!(LanguageMap::default().string_count(), 0);
        pretty_assertions::assert_eq!(LanguageMapValue::Single("a".to_owned()).string_count(), 1);
        pretty_assertions::assert_eq!(LanguageMapValue::Multiple(Vec::new()).string_count(), 0);
    }

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
        pretty_assertions::assert_eq!(property.value_type.as_ref().map(CoercedTerm::as_str), Some("langString"));
    }

    #[test]
    fn the_language_property_default_matches_the_field_by_field_literal() {
        let actual = LanguageProperty::default();
        assert!(actual.language_map.is_empty());
        assert!(actual.value_type.is_none() && actual.previous_language_map.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }

    #[test]
    fn a_language_map_reads_each_entry_as_a_string_or_an_array_of_strings() {
        let map: LanguageMap = serde_json::from_str(r#"{"en": "hello", "@none": ["a", "b"]}"#).expect("a language map");
        pretty_assertions::assert_eq!(
            map.get(&LanguageMapKey::Tag("en".parse().expect("a tag"))),
            Some(&LanguageMapValue::Single("hello".to_owned()))
        );
        pretty_assertions::assert_eq!(
            map.get(&LanguageMapKey::None),
            Some(&LanguageMapValue::Multiple(vec!["a".to_owned(), "b".to_owned()]))
        );
    }

    #[test]
    fn a_language_map_entry_of_another_kind_is_refused_naming_its_tag() {
        for written in [r#"{"en": 42}"#, r#"{"en": {"text": "hello"}}"#, r#"{"en": null}"#] {
            let refused = serde_json::from_str::<LanguageMap>(written).expect_err(written);
            assert!(
                refused
                    .to_string()
                    .contains("expected a string or an array of strings under the language tag 'en'"),
                "{refused}"
            );
        }
    }
}
