//! Language negotiation over BCP 47 ranges (clause 5.7.2.5).
//!
//! The same value reaches a request three ways -- the `lang` query parameter
//! of clause 4.15, the `NGSILD-Language` header and, under the
//! `accept_language` extension, `Accept-Language` itself -- and all three spell
//! it with the `Accept-Language` syntax of RFC 9110 section 12.5.4, so the
//! grammar is owned here rather than by any carrier.

use crate::quality::{Quality, QualityParseError, QualityValue};
use http::{
    HeaderValue,
    header::{InvalidHeaderValue, ToStrError},
};
use langtag::{InvalidLangTag, LangTag, LangTagBuf};
use std::{cmp::Reverse, fmt};
use thiserror::Error;

/// A language preference: either a wildcard or a typed BCP 47 language tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageRange {
    /// Wildcard `*`, matching any available language.
    Wildcard,
    /// A validated BCP 47 language tag.
    Tag(LangTagBuf),
}

impl LanguageRange {
    /// Parse a language range: either `"*"` or a valid BCP 47 tag.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageFilterError::InvalidLanguageTag`] if the input is
    /// neither `"*"` nor a valid BCP 47 tag.
    pub fn parse(raw: &str) -> Result<LanguageRange, LanguageFilterError> {
        if raw == "*" {
            return Ok(LanguageRange::Wildcard);
        }

        let tag = LangTagBuf::new(raw.to_string()).map_err(LanguageFilterError::InvalidLanguageTag)?;

        Ok(LanguageRange::Tag(tag))
    }

    /// Parse one preference segment, such as `"fr;q=0.9"`.
    ///
    /// A range followed by an optional `;q=` quality parameter. Anything other
    /// than `q` is rejected rather than ignored, because a parameter this
    /// server does not understand may change what the client meant.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageFilterError`] if the segment is empty, the language
    /// tag is invalid, or the quality parameter is malformed.
    pub fn parse_ranked(raw: &str) -> Result<QualityValue<LanguageRange>, LanguageFilterError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(LanguageFilterError::EmptyPreference);
        }

        let mut segments = trimmed.split(';');
        let range = LanguageRange::parse(segments.next().ok_or(LanguageFilterError::EmptyPreference)?.trim())?;
        let mut quality = Quality::default();

        for parameter in segments {
            let parameter = parameter.trim();
            if parameter.is_empty() {
                return Err(LanguageFilterError::MalformedParameter);
            }

            let (name, value) = parameter.split_once('=').ok_or(LanguageFilterError::MalformedParameter)?;

            if name.trim() != "q" {
                return Err(LanguageFilterError::UnsupportedParameter);
            }

            quality = Quality::parse_decimal(value.trim()).map_err(LanguageFilterError::InvalidQuality)?;
        }

        Ok(QualityValue::new(range, quality))
    }

    /// Case-insensitive exact match against an available tag.
    ///
    /// A wildcard matches any tag; a tag range matches only an equal tag,
    /// case-insensitively per BCP 47.
    #[must_use]
    pub fn exact_matches(&self, available: &LangTag) -> bool {
        match self {
            LanguageRange::Wildcard => true,
            LanguageRange::Tag(requested) => {
                let requested: &LangTag = requested;
                requested == available
            }
        }
    }

    /// RFC 4647 section 3.3 basic filtering prefix match.
    ///
    /// True when this range is a case-insensitive prefix of `available`
    /// followed by a `-` subtag separator. Exact matches are excluded -- use
    /// [`exact_matches`](Self::exact_matches) for those.
    #[must_use]
    pub fn prefix_matches(&self, available: &LangTag) -> bool {
        match self {
            LanguageRange::Wildcard => false,
            LanguageRange::Tag(requested) => {
                let range = requested.as_str();
                let tag = available.as_str();
                tag.len() > range.len() && tag.as_bytes()[range.len()] == b'-' && tag[..range.len()].eq_ignore_ascii_case(range)
            }
        }
    }
}

impl fmt::Display for LanguageRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LanguageRange::Wildcard => f.write_str("*"),
            LanguageRange::Tag(tag) => fmt::Display::fmt(tag, f),
        }
    }
}

/// A non-empty, quality-ranked list of language ranges.
///
/// Non-emptiness is the type's invariant rather than a caller's obligation:
/// every constructor rejects an empty list, so a filter that exists always has
/// something to negotiate against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageFilter {
    /// The ranges as written, in wire order; never empty.
    preferences: Vec<QualityValue<LanguageRange>>,
}

impl LanguageFilter {
    /// Build a single-preference filter from an already validated BCP 47 tag.
    #[must_use]
    pub fn from_tag(tag: LangTagBuf) -> LanguageFilter {
        LanguageFilter {
            preferences: vec![QualityValue::new(LanguageRange::Tag(tag), Quality::default())],
        }
    }

    /// Parse a comma-separated preference list (clause 4.15).
    ///
    /// # Errors
    ///
    /// Returns [`LanguageFilterError`] if the input is empty, contains invalid
    /// language tags, or has malformed quality parameters.
    pub fn parse(raw: &str) -> Result<LanguageFilter, LanguageFilterError> {
        LanguageFilter::new(raw.split(',').map(LanguageRange::parse_ranked).collect::<Result<Vec<_>, _>>()?)
    }

    /// Read the preference list a header spells with the `Accept-Language`
    /// syntax, across every line it was sent on.
    ///
    /// Empty list elements are skipped, as RFC 9110 section 5.6.1 requires of
    /// a list recipient, so a header naming no language is `Ok(None)`: it
    /// states no preference rather than an invalid one.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageFilterError::NotVisibleText`] for a value that is not
    /// visible ASCII, and the error of the first element that is not a
    /// language range with an optional weight.
    pub fn from_header_values<'i, I>(values: I) -> Result<Option<LanguageFilter>, LanguageFilterError>
    where
        I: IntoIterator<Item = &'i HeaderValue>,
    {
        let mut preferences = Vec::new();
        for value in values {
            let raw = value.to_str().map_err(LanguageFilterError::NotVisibleText)?;
            for element in raw.split(',').map(str::trim).filter(|element| !element.is_empty()) {
                preferences.push(LanguageRange::parse_ranked(element)?);
            }
        }
        Ok((!preferences.is_empty()).then_some(LanguageFilter { preferences }))
    }

    /// The preferences as one header value in the `Accept-Language` syntax,
    /// in wire order.
    ///
    /// # Errors
    ///
    /// Returns the [`InvalidHeaderValue`] of a rendering that is no header
    /// value, which a filter built from validated tags never produces.
    pub fn to_header_value(&self) -> Result<HeaderValue, InvalidHeaderValue> {
        HeaderValue::from_str(&self.preferences.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
    }

    /// Build a filter from already-parsed preferences.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageFilterError::Empty`] if the preference list is empty.
    pub fn new(preferences: Vec<QualityValue<LanguageRange>>) -> Result<LanguageFilter, LanguageFilterError> {
        if preferences.is_empty() {
            return Err(LanguageFilterError::Empty);
        }
        Ok(LanguageFilter { preferences })
    }

    /// The preferences as written, in wire order.
    #[must_use]
    pub fn preferences(&self) -> &[QualityValue<LanguageRange>] {
        &self.preferences
    }

    /// The preferences by descending quality, ties broken by wire order.
    ///
    /// Zero-quality preferences are excluded: RFC 9110 gives `q=0` the meaning
    /// "not acceptable", so a language named that way must not be selected.
    #[must_use]
    pub fn ordered(&self) -> Vec<&QualityValue<LanguageRange>> {
        let mut preferences = self
            .preferences
            .iter()
            .enumerate()
            .filter(|(_, preference)| preference.quality().value() > 0)
            .collect::<Vec<_>>();
        preferences.sort_by_key(|(ordinal, preference)| (Reverse(preference.quality()), *ordinal));
        preferences.into_iter().map(|(_, preference)| preference).collect()
    }
}

/// Typed parse errors for a language filter.
#[derive(Debug, Error)]
pub enum LanguageFilterError {
    /// The preference list was empty.
    #[error("the `lang` query parameter cannot be empty")]
    Empty,
    /// One of the comma-delimited preferences was empty.
    #[error("the `lang` query parameter contains an empty language preference")]
    EmptyPreference,
    /// A parameter segment is empty or not a `name=value` pair.
    #[error("a language preference parameter is not a name=value pair")]
    MalformedParameter,
    /// A parameter other than `q` was provided.
    #[error("a language preference accepts only the q parameter")]
    UnsupportedParameter,
    /// A `q` value was syntactically invalid.
    #[error("invalid language preference quality value")]
    InvalidQuality(#[source] QualityParseError),
    /// A language tag was invalid.
    #[error("invalid language tag")]
    InvalidLanguageTag(#[source] InvalidLangTag<String>),
    /// A header carrying the list is not visible ASCII text.
    #[error("a language preference header is not visible ASCII text")]
    NotVisibleText(#[source] ToStrError),
}

#[cfg(test)]
mod tests {
    use crate::{
        language::{LanguageFilter, LanguageFilterError, LanguageRange},
        quality::{Quality, QualityParseError, QualityValue},
    };
    use http::HeaderValue;
    use langtag::{LangTag, LangTagBuf};

    fn range(raw: &str) -> LanguageRange {
        LanguageRange::Tag(LangTagBuf::new(raw.to_string()).expect("valid tag"))
    }

    fn tag(raw: &str) -> &LangTag {
        LangTag::new(raw).expect("valid tag")
    }

    fn ranked(range: LanguageRange, quality: u16) -> QualityValue<LanguageRange> {
        QualityValue::new(range, Quality::new(quality).expect("in range"))
    }

    #[test]
    fn a_wildcard_matches_any_tag_exactly_and_none_by_prefix() {
        assert!(LanguageRange::Wildcard.exact_matches(tag("fr")));
        assert!(LanguageRange::Wildcard.exact_matches(tag("en-US")));
        assert!(!LanguageRange::Wildcard.prefix_matches(tag("fr")));
    }

    #[test]
    fn an_exact_match_ignores_case_but_not_subtags() {
        assert!(range("fr-CA").exact_matches(tag("fr-CA")));
        assert!(range("fr-CA").exact_matches(tag("FR-ca")));
        assert!(!range("fr").exact_matches(tag("fr-CA")));
    }

    #[test]
    fn a_prefix_match_requires_a_subtag_separator() {
        assert!(range("fr").prefix_matches(tag("fr-CA")));
        assert!(!range("fr").prefix_matches(tag("fra")));
        assert!(!range("fr").prefix_matches(tag("de")));
    }

    #[test]
    fn ordering_is_by_descending_quality_then_wire_order() {
        let filter = LanguageFilter::new(vec![
            ranked(range("fr-CH"), 1000),
            ranked(range("fr"), 900),
            ranked(range("en"), 800),
            ranked(LanguageRange::Wildcard, 500),
        ])
        .expect("non-empty");

        let ordered = filter.ordered();
        let rendered: Vec<String> = ordered.iter().map(|preference| preference.value().to_string()).collect();
        pretty_assertions::assert_eq!(rendered, vec!["fr-CH", "fr", "en", "*"]);
    }

    #[test]
    fn equal_quality_preserves_the_order_written() {
        let filter = LanguageFilter::new(vec![ranked(range("de"), 1000), ranked(range("en"), 1000), ranked(range("fr"), 1000)]).expect("non-empty");

        let rendered: Vec<String> = filter.ordered().iter().map(|preference| preference.value().to_string()).collect();
        pretty_assertions::assert_eq!(rendered, vec!["de", "en", "fr"]);
    }

    #[test]
    fn a_zero_quality_preference_is_not_offered() {
        let filter = LanguageFilter::new(vec![ranked(range("de"), 0), ranked(range("en"), 800)]).expect("non-empty");

        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered.len(), 1);
        pretty_assertions::assert_eq!(ordered[0].value().to_string(), "en");
        pretty_assertions::assert_eq!(filter.preferences().len(), 2);
    }

    #[test]
    fn an_empty_preference_list_is_not_a_filter() {
        assert!(matches!(LanguageFilter::new(vec![]), Err(LanguageFilterError::Empty)));
    }

    #[test]
    fn a_single_tag_parses_at_the_default_quality() {
        let filter = LanguageFilter::parse("fr").expect("valid");
        pretty_assertions::assert_eq!(filter.preferences().len(), 1);
        pretty_assertions::assert_eq!(filter.preferences()[0].value().to_string(), "fr");
        pretty_assertions::assert_eq!(filter.preferences()[0].quality(), Quality::MAX);
    }

    #[test]
    fn a_ranked_list_parses_in_wire_order() {
        let filter = LanguageFilter::parse("fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5").expect("valid");
        let rendered: Vec<String> = filter.ordered().iter().map(|preference| preference.value().to_string()).collect();
        pretty_assertions::assert_eq!(rendered, vec!["fr-CH", "fr", "en", "*"]);
    }

    #[test]
    fn an_empty_segment_is_rejected_rather_than_skipped() {
        assert!(matches!(LanguageFilter::parse(""), Err(LanguageFilterError::EmptyPreference)));
        assert!(matches!(LanguageFilter::parse("fr,,en"), Err(LanguageFilterError::EmptyPreference)));
    }

    #[test]
    fn a_malformed_quality_parameter_is_rejected() {
        assert!(matches!(
            LanguageFilter::parse("fr;q=1.1"),
            Err(LanguageFilterError::InvalidQuality(QualityParseError::AboveOne))
        ));
        assert!(matches!(LanguageFilter::parse("fr;q"), Err(LanguageFilterError::MalformedParameter)));
        assert!(matches!(LanguageFilter::parse("fr;"), Err(LanguageFilterError::MalformedParameter)));
    }

    #[test]
    fn a_parameter_other_than_quality_is_rejected() {
        assert!(matches!(LanguageFilter::parse("fr;level=1"), Err(LanguageFilterError::UnsupportedParameter)));
    }

    fn header_values(lines: &[&'static str]) -> Vec<HeaderValue> {
        lines.iter().map(|line| HeaderValue::from_static(line)).collect()
    }

    #[test]
    fn a_header_list_is_read_across_every_line_in_wire_order() {
        let filter = LanguageFilter::from_header_values(&header_values(&["fr-CH, fr;q=0.9", "en;q=0.8, *;q=0.5"]))
            .expect("valid")
            .expect("a preference");
        let rendered: Vec<String> = filter.preferences().iter().map(|preference| preference.value().to_string()).collect();
        pretty_assertions::assert_eq!(rendered, vec!["fr-CH", "fr", "en", "*"]);
    }

    #[test]
    fn a_header_list_skips_empty_elements() {
        let filter = LanguageFilter::from_header_values(&header_values(&[" , fr ,, en;q=0.5 ,"]))
            .expect("valid")
            .expect("a preference");
        pretty_assertions::assert_eq!(filter.preferences().len(), 2);
    }

    #[test]
    fn a_header_list_naming_no_language_states_no_preference() {
        assert!(LanguageFilter::from_header_values(&header_values(&[""])).expect("valid").is_none());
        assert!(LanguageFilter::from_header_values(&header_values(&[" , "])).expect("valid").is_none());
        assert!(LanguageFilter::from_header_values(&[]).expect("valid").is_none());
    }

    #[test]
    fn a_header_list_with_an_invalid_element_is_rejected() {
        assert!(matches!(
            LanguageFilter::from_header_values(&header_values(&["fr, en_US"])),
            Err(LanguageFilterError::InvalidLanguageTag(_))
        ));
        assert!(matches!(
            LanguageFilter::from_header_values(&header_values(&["fr;level=1"])),
            Err(LanguageFilterError::UnsupportedParameter)
        ));
    }

    #[test]
    fn a_header_value_that_is_not_visible_text_is_rejected() {
        let value = HeaderValue::from_bytes(b"fr\xe9").expect("an opaque header value");
        assert!(matches!(
            LanguageFilter::from_header_values([&value]),
            Err(LanguageFilterError::NotVisibleText(_))
        ));
    }

    #[test]
    fn the_header_value_lists_the_preferences_in_wire_order() {
        let filter = LanguageFilter::parse("en;q=0.5,fr,*;q=0.1").expect("valid");
        pretty_assertions::assert_eq!(filter.to_header_value().expect("a header value"), "en; q=0.5, fr, *; q=0.1");
    }

    #[test]
    fn an_invalid_language_tag_is_rejected() {
        assert!(matches!(LanguageFilter::parse("123"), Err(LanguageFilterError::InvalidLanguageTag(_))));
    }
}
