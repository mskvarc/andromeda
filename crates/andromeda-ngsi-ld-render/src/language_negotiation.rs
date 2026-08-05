//! Clause 4.5.6 language negotiation over a `LanguageProperty`.
//!
//! When a request carries a `lang` filter, a `LanguageProperty` is not
//! rendered as itself: clause 4.5.6 says the broker picks one entry from the
//! language map by RFC 4647 basic filtering and answers with a `Property`
//! carrying that entry plus a `lang` member naming the tag it chose. Every
//! other attribute type is unaffected, which is why the dispatch below
//! enumerates the eight types rather than testing for a language map.

use crate::{entity::serialize_value, error::RepresentationError};
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
pub(crate) fn render_attribute_value(attr: &AttributeValue, language_filter: Option<&LanguageFilter>) -> Result<Value, RepresentationError> {
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
fn render_attribute(attribute: &Attribute, language_filter: Option<&LanguageFilter>) -> Result<Value, RepresentationError> {
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

/// Render a `LanguageProperty` as the `Property` clause 4.5.6 asks for.
fn render_language_property(language_property: &LanguageProperty, language_filter: Option<&LanguageFilter>) -> Result<Value, RepresentationError> {
    if let Some(filter) = language_filter
        && let Some(negotiated) = negotiate_language_property(language_property, filter)
    {
        return serialize_value(&negotiated);
    }

    // Clone required: Attribute's #[serde(tag = "type")] adds the "type" discriminant during serialization.
    // Serializing LanguageProperty directly would omit the type tag.
    serialize_value(&Attribute::LanguageProperty(language_property.clone()))
}

/// Pick the language-map entry the filter selects, by RFC 4647 basic filtering.
fn negotiate_language_property<'a>(language_property: &'a LanguageProperty, filter: &LanguageFilter) -> Option<NegotiatedProperty<'a>> {
    let available = language_entries(&language_property.language_map);

    for preference in filter.ordered() {
        let range = preference.value();
        match range {
            LanguageRange::Wildcard => {
                if let Some(entry) = available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Tag(_))) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
                if let Some(entry) = available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Default)) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
            }
            LanguageRange::Tag(_) => {
                // Exact match
                if let Some(entry) = available.iter().find(|entry| entry.matches_exact(range)) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
                // Prefix match (only for basic ranges like "fr", not "fr-CA")
                if range.is_basic_range()
                    && let Some(entry) = available.iter().find(|entry| entry.matches_prefix(range))
                {
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
/// Clause 4.5.6: the selected entry becomes the value, the tag it came from
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
