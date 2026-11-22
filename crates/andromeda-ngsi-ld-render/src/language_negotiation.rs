//! Clause 4.15 language negotiation over a `LanguageProperty`.
//!
//! When a request carries a `lang` filter, a `LanguageProperty` of an Entity is
//! not rendered as itself: clause 4.15 says the broker picks one entry from the
//! language map by RFC 4647 basic filtering and answers with a `Property`
//! carrying that entry plus a `lang` member naming the tag it chose. This
//! module picks the entry; the entity renderers write the `Property` from it.

use andromeda_http_headers::language::{LanguageFilter, LanguageRange};
use andromeda_ngsi_ld_model::attribute::language_property::{LanguageMapKey, LanguageMapValue, LanguageProperty};
use indexmap::IndexMap;
use langtag::LangTagBuf;

/// The language-map entry clause 4.15 answers a `LanguageProperty` with under
/// a `lang` filter: the entry the filter selects by RFC 4647 basic filtering,
/// or, when nothing matches, the unmatched-language default.
///
/// `None` only when the language map is empty, which has nothing to serve and
/// leaves the property as itself.
pub(crate) fn negotiated_entry<'a>(language_property: &'a LanguageProperty, filter: &LanguageFilter) -> Option<NegotiatedEntry<'a>> {
    let available = language_entries(&language_property.language_map);
    negotiate_entry(&available, filter)
        .or_else(|| unmatched_language_entry(&available))
        .map(AvailableLanguage::entry)
}

/// The entry served when no range of the filter matches: the `@none` default
/// when present, else a tagged entry. Clause 4.15 lets the broker default to any
/// supported language, and clause 5.7.2.5 makes it the `@none` entry if present.
fn unmatched_language_entry<'a, 'b>(available: &'b [AvailableLanguage<'a>]) -> Option<&'b AvailableLanguage<'a>> {
    available
        .iter()
        .find(|entry| matches!(entry.range, AvailableLanguageRange::Default))
        .or_else(|| available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Tag(_))))
}

/// The entry clause 4.15 selects out of a language map.
#[derive(Debug)]
pub(crate) struct NegotiatedEntry<'a> {
    /// The tag the entry is keyed by, or `@none`.
    pub(crate) lang: LanguageMapKey,
    /// The text the entry carries.
    pub(crate) value: &'a LanguageMapValue,
}

/// The entry the wildcard range selects: a tagged one by preference, since
/// RFC 4647 `*` matches any language tag, else the `@none` default.
fn wildcard_entry<'a, 'b>(available: &'b [AvailableLanguage<'a>]) -> Option<&'b AvailableLanguage<'a>> {
    available
        .iter()
        .find(|entry| matches!(entry.range, AvailableLanguageRange::Tag(_)))
        .or_else(|| available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Default)))
}

/// Pick the language-map entry the filter selects, by RFC 4647 basic filtering.
fn negotiate_entry<'a, 'b>(available: &'b [AvailableLanguage<'a>], filter: &LanguageFilter) -> Option<&'b AvailableLanguage<'a>> {
    for preference in filter.ordered() {
        let range = preference.value();
        match range {
            LanguageRange::Wildcard => {
                if let Some(entry) = wildcard_entry(available) {
                    return Some(entry);
                }
            }
            LanguageRange::Tag(_) => {
                if let Some(entry) = available.iter().find(|entry| entry.matches_exact(range)) {
                    return Some(entry);
                }
                // Prefix match. RFC 4647 basic filtering places no restriction to
                // basic ranges, so an extended range like `fr-CA` still matches a
                // longer tag such as `fr-CA-1994`.
                if let Some(entry) = available.iter().find(|entry| entry.matches_prefix(range)) {
                    return Some(entry);
                }
            }
        }
    }

    None
}

/// The language map read as a list of what is available to select from.
fn language_entries(language_map: &IndexMap<LanguageMapKey, LanguageMapValue>) -> Vec<AvailableLanguage<'_>> {
    language_map.iter().map(|(key, value)| AvailableLanguage::from_entry(key, value)).collect()
}

#[derive(Debug)]
/// One entry of a language map, as a candidate for negotiation.
struct AvailableLanguage<'a> {
    /// The tag this entry is keyed by, or the absence of one.
    range: AvailableLanguageRange,
    /// The text this entry carries.
    value: &'a LanguageMapValue,
}

impl<'a> AvailableLanguage<'a> {
    /// Construct from a typed language map entry. Infallible because keys are
    /// already validated at deserialization time.
    fn from_entry(key: &LanguageMapKey, value: &'a LanguageMapValue) -> AvailableLanguage<'a> {
        let range = match key {
            LanguageMapKey::None => AvailableLanguageRange::Default,
            LanguageMapKey::Tag(tag) => AvailableLanguageRange::Tag(tag.clone()),
        };
        AvailableLanguage { range, value }
    }

    /// Case-insensitive exact match using [`LanguageRange::exact_matches`].
    fn matches_exact(&self, range: &LanguageRange) -> bool {
        match &self.range {
            AvailableLanguageRange::Tag(tag) => range.exact_matches(tag),
            AvailableLanguageRange::Default => false,
        }
    }

    /// The negotiated entry this candidate answers with.
    fn entry(&self) -> NegotiatedEntry<'a> {
        NegotiatedEntry {
            lang: match &self.range {
                AvailableLanguageRange::Default => LanguageMapKey::None,
                AvailableLanguageRange::Tag(tag) => LanguageMapKey::Tag(tag.clone()),
            },
            value: self.value,
        }
    }

    /// RFC 4647 basic filtering prefix match using [`LanguageRange::prefix_matches`].
    fn matches_prefix(&self, range: &LanguageRange) -> bool {
        match &self.range {
            AvailableLanguageRange::Tag(tag) => range.prefix_matches(tag),
            AvailableLanguageRange::Default => false,
        }
    }
}

#[derive(Debug, Clone)]
/// What a language-map key names.
enum AvailableLanguageRange {
    /// The `@none` key: text with no language stated.
    Default,
    /// A BCP 47 language tag.
    Tag(LangTagBuf),
}

#[cfg(test)]
mod tests {
    use crate::language_negotiation::negotiated_entry;
    use andromeda_http_headers::language::LanguageFilter;
    use andromeda_ngsi_ld_model::attribute::{instance::Attribute, language_property::LanguageProperty};
    use serde_json::{Value, json};

    /// A `LanguageProperty` carrying `map` as its language map.
    fn language_property(map: &Value) -> LanguageProperty {
        let attribute: Attribute = serde_json::from_value(json!({"type": "LanguageProperty", "languageMap": map})).expect("a language property");
        let Attribute::LanguageProperty(language_property) = attribute else {
            panic!("a LanguageProperty reads as a LanguageProperty");
        };
        language_property
    }

    /// A language filter parsed from a `lang` value.
    fn filter(raw: &str) -> LanguageFilter {
        LanguageFilter::parse(raw).expect("a valid language filter")
    }

    /// The text and the tag of the entry `lang` selects, as written.
    fn negotiated(map: &Value, lang: &str) -> (Value, Value) {
        let language_property = language_property(map);
        let entry = negotiated_entry(&language_property, &filter(lang)).expect("an entry is served");
        (
            serde_json::to_value(entry.value).expect("the text writes"),
            serde_json::to_value(&entry.lang).expect("the tag writes"),
        )
    }

    #[test]
    fn a_matching_language_is_served_directly() {
        let (value, lang) = negotiated(&json!({"fr": "Grand Place", "en": "Great Square"}), "en");
        pretty_assertions::assert_eq!(value, "Great Square");
        pretty_assertions::assert_eq!(lang, "en");
    }

    #[test]
    fn no_matching_language_defaults_to_a_supported_one() {
        // Clause 4.15: with no @none entry, any supported language is served.
        let (value, lang) = negotiated(&json!({"fr": "Grand Place", "nl": "Grote Markt"}), "de");
        pretty_assertions::assert_eq!(value, "Grand Place");
        pretty_assertions::assert_eq!(lang, "fr");
    }

    #[test]
    fn an_extended_range_prefix_matches_a_longer_tag() {
        // RFC 4647 basic filtering places no basic-range restriction, so `fr-CA`
        // matches the longer tag `fr-CA-1994`.
        let (value, lang) = negotiated(&json!({"fr-CA-1994": "quebecois"}), "fr-CA");
        pretty_assertions::assert_eq!(value, "quebecois");
        pretty_assertions::assert_eq!(lang, "fr-CA-1994");
    }

    #[test]
    fn no_match_defaults_to_the_none_entry_when_only_it_is_present() {
        let (value, lang) = negotiated(&json!({"@none": "untagged"}), "de");
        pretty_assertions::assert_eq!(value, "untagged");
        pretty_assertions::assert_eq!(lang, "@none");
    }

    /// Clause 5.7.2.5: with no match, the `@none` default is chosen if present.
    #[test]
    fn no_match_prefers_the_none_entry_over_a_tagged_one() {
        let (value, lang) = negotiated(&json!({"en": "red", "@none": "x"}), "de");
        pretty_assertions::assert_eq!(value, "x");
        pretty_assertions::assert_eq!(lang, "@none");
    }

    /// RFC 4647 `*` matches any language tag, so a tagged entry wins over `@none`.
    #[test]
    fn the_wildcard_prefers_a_tagged_entry_over_the_none_entry() {
        let (value, lang) = negotiated(&json!({"@none": "x", "en": "red"}), "*");
        pretty_assertions::assert_eq!(value, "red");
        pretty_assertions::assert_eq!(lang, "en");
    }
}
