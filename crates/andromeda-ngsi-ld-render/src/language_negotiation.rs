//! Clause 4.15 language negotiation over a `LanguageProperty`.
//!
//! When a request carries a `lang` filter, a `LanguageProperty` is not
//! rendered as itself: clause 4.15 says the broker picks one entry from the
//! language map by RFC 4647 basic filtering and answers with a `Property`
//! carrying that entry plus a `lang` member naming the tag it chose. Every
//! other attribute type is unaffected, which is why the dispatch below
//! enumerates the eight types rather than testing for a language map.

use crate::{entity::serialize_value, error::RenderError};
use andromeda_ngsi_ld_http_headers::language::{LanguageFilter, LanguageRange};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, rfc3339};
use andromeda_ngsi_ld_model::attribute::{
    instance::{Attribute, AttributeValue},
    language_property::{LanguageMapKey, LanguageMapValue, LanguageProperty},
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use langtag::LangTagBuf;
use serde::Serialize;
use serde_json::Value;
use std::ops::Deref;
use urn_rs::Urn;

/// Render one attribute value, negotiating any language map inside it.
pub(crate) fn render_attribute_value(attr: &AttributeValue, language_filter: Option<&LanguageFilter>) -> Result<Value, RenderError> {
    match attr {
        AttributeValue::Single(attribute) => render_attribute(attribute.deref(), language_filter),
        AttributeValue::Multiple(attributes) => attributes
            .iter()
            .map(|attribute| render_attribute(attribute, language_filter))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        AttributeValue::Raw(value) => Ok(value.clone()),
    }
}

/// Render one attribute instance, negotiating a language map if it is one.
fn render_attribute(attribute: &Attribute, language_filter: Option<&LanguageFilter>) -> Result<Value, RenderError> {
    match attribute {
        Attribute::LanguageProperty(language_property) => render_language_property(language_property, language_filter),
        Attribute::Property(_)
        | Attribute::Relationship(_)
        | Attribute::VocabProperty(_)
        | Attribute::GeoProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::ListRelationship(_) => serialize_value(attribute),
    }
}

/// Render a `LanguageProperty` as the `Property` clause 4.15 asks for.
///
/// With a `lang` filter present the answer is always a `Property`: the entry the
/// filter selects, or — when nothing matches — any supported language, because
/// clause 4.15 states the broker *shall* default to a supported language rather
/// than return the raw language map. Only the empty language map, or the absence
/// of a filter, leaves the property as itself.
fn render_language_property(language_property: &LanguageProperty, language_filter: Option<&LanguageFilter>) -> Result<Value, RenderError> {
    if let Some(filter) = language_filter {
        let selected = negotiate_language_property(language_property, filter).or_else(|| default_language_property(language_property));
        if let Some(negotiated) = selected {
            return serialize_value(&negotiated);
        }
    }

    // Clone required: Attribute's #[serde(tag = "type")] adds the "type" discriminant during serialization.
    // Serializing LanguageProperty directly would omit the type tag.
    serialize_value(&Attribute::LanguageProperty(language_property.clone()))
}

/// The `Property` clause 4.15 defaults to when no requested language matches:
/// any supported language — the first tagged entry, else the `@none` default.
///
/// `None` only when the language map is empty, which has nothing to serve.
fn default_language_property(language_property: &LanguageProperty) -> Option<NegotiatedProperty<'_>> {
    let available = language_entries(&language_property.language_map);
    let selected = default_supported_entry(&available)?;
    Some(NegotiatedProperty::from_language_property(language_property, selected))
}

/// Any supported entry of a language map: a tagged one by preference, else the
/// `@none` default. The clause 4.15 fallback target, shared by the wildcard
/// range and the no-match default so the two cannot diverge.
fn default_supported_entry<'a, 'b>(available: &'b [AvailableLanguage<'a>]) -> Option<&'b AvailableLanguage<'a>> {
    available
        .iter()
        .find(|entry| matches!(entry.range, AvailableLanguageRange::Tag(_)))
        .or_else(|| available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Default)))
}

/// Pick the language-map entry the filter selects, by RFC 4647 basic filtering.
fn negotiate_language_property<'a>(language_property: &'a LanguageProperty, filter: &LanguageFilter) -> Option<NegotiatedProperty<'a>> {
    let available = language_entries(&language_property.language_map);

    for preference in filter.ordered() {
        let range = preference.value();
        match range {
            LanguageRange::Wildcard => {
                if let Some(entry) = default_supported_entry(&available) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
            }
            LanguageRange::Tag(_) => {
                // Exact match.
                if let Some(entry) = available.iter().find(|entry| entry.matches_exact(range)) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
                // Prefix match. RFC 4647 basic filtering places no restriction to
                // basic ranges, so an extended range like `fr-CA` still matches a
                // longer tag such as `fr-CA-1994`.
                if let Some(entry) = available.iter().find(|entry| entry.matches_prefix(range)) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// The `Property` a negotiated `LanguageProperty` is answered as.
///
/// Clause 4.15: the selected entry becomes the value, the tag it came from
/// becomes a `lang` member, and everything else about the attribute survives.
struct NegotiatedProperty<'a> {
    #[serde(rename = "type")]
    /// Always `Property`: negotiation replaces the language map with one value.
    type_: &'static str,
    /// The text selected out of the language map.
    value: &'a LanguageMapValue,
    /// The tag it was keyed by, or `@none`.
    lang: String,
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    /// The observation instant, when the attribute carried one.
    observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// The dataset this instance belongs to, when it names one.
    dataset_id: &'a Option<Urn>,
    #[serde(flatten)]
    /// The attribute's own sub-attributes, flattened in beside it.
    sub_attributes: &'a IndexMap<AttributeName, AttributeValue>,
}

impl<'a> NegotiatedProperty<'a> {
    /// Build the answer from a language property and the entry chosen from it.
    fn from_language_property(language_property: &'a LanguageProperty, selected: &AvailableLanguage<'a>) -> NegotiatedProperty<'a> {
        NegotiatedProperty {
            type_: "Property",
            value: selected.value,
            lang: match &selected.range {
                AvailableLanguageRange::Default => "@none".to_string(),
                AvailableLanguageRange::Tag(tag) => tag.to_string(),
            },
            observed_at: language_property.common.observed_at,
            dataset_id: &language_property.common.dataset_id,
            sub_attributes: &language_property.common.sub_attributes,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::language_negotiation::render_attribute_value;
    use andromeda_ngsi_ld_http_headers::language::LanguageFilter;
    use andromeda_ngsi_ld_model::attribute::instance::AttributeValue;
    use serde_json::{Value, json};

    /// A `LanguageProperty` attribute value carrying `map` as its language map.
    fn language_attr(map: Value) -> AttributeValue {
        serde_json::from_value(json!({"type": "LanguageProperty", "languageMap": map})).expect("a language property")
    }

    /// A language filter parsed from a `lang` value.
    fn filter(raw: &str) -> LanguageFilter {
        LanguageFilter::parse(raw).expect("a valid language filter")
    }

    #[test]
    fn a_matching_language_is_served_directly() {
        let attr = language_attr(json!({"fr": "Grand Place", "en": "Great Square"}));
        let rendered = render_attribute_value(&attr, Some(&filter("en"))).expect("render");
        pretty_assertions::assert_eq!(rendered["type"], "Property");
        pretty_assertions::assert_eq!(rendered["value"], "Great Square");
        pretty_assertions::assert_eq!(rendered["lang"], "en");
    }

    #[test]
    fn no_matching_language_defaults_to_a_supported_one() {
        // Clause 4.15: the broker shall default to any supported language rather
        // than return the raw language map. The first tagged entry is served.
        let attr = language_attr(json!({"fr": "Grand Place", "nl": "Grote Markt"}));
        let rendered = render_attribute_value(&attr, Some(&filter("de"))).expect("render");
        pretty_assertions::assert_eq!(rendered["type"], "Property");
        pretty_assertions::assert_eq!(rendered["value"], "Grand Place");
        pretty_assertions::assert_eq!(rendered["lang"], "fr");
    }

    #[test]
    fn an_extended_range_prefix_matches_a_longer_tag() {
        // RFC 4647 basic filtering places no basic-range restriction, so `fr-CA`
        // matches the longer tag `fr-CA-1994`.
        let attr = language_attr(json!({"fr-CA-1994": "quebecois"}));
        let rendered = render_attribute_value(&attr, Some(&filter("fr-CA"))).expect("render");
        pretty_assertions::assert_eq!(rendered["value"], "quebecois");
        pretty_assertions::assert_eq!(rendered["lang"], "fr-CA-1994");
    }

    #[test]
    fn no_match_defaults_to_the_none_entry_when_only_it_is_present() {
        // With no tagged entry, the `@none` default is the supported fallback.
        let attr = language_attr(json!({"@none": "untagged"}));
        let rendered = render_attribute_value(&attr, Some(&filter("de"))).expect("render");
        pretty_assertions::assert_eq!(rendered["value"], "untagged");
        pretty_assertions::assert_eq!(rendered["lang"], "@none");
    }
}
