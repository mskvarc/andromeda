//! Language negotiation over BCP 47 ranges (clause 5.7.2.5).
//!
//! The same value reaches a request two ways -- the `lang` query parameter of
//! clause 4.15 and the `NGSILD-Language` header -- and both spell it with the
//! `Accept-Language` syntax of RFC 7231 section 5.3.5, so the grammar is owned
//! here rather than by either carrier.

use crate::quality::{Quality, QualityValue};
use langtag::{LangTag, LangTagBuf};
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

        let tag = LangTagBuf::new(raw.to_string()).map_err(|error| LanguageFilterError::InvalidLanguageTag {
            value: raw.to_string(),
            source: error,
        })?;

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
                return Err(LanguageFilterError::MalformedParameter(trimmed.to_string()));
            }

            let (name, value) = parameter
                .split_once('=')
                .ok_or_else(|| LanguageFilterError::MalformedParameter(parameter.to_string()))?;

            if name.trim() != "q" {
                return Err(LanguageFilterError::UnsupportedParameter(name.trim().to_string()));
            }

            quality = Quality::parse_decimal(value.trim()).map_err(|_| LanguageFilterError::InvalidQuality(value.trim().to_string()))?;
        }

        Ok(QualityValue::new(range, quality))
    }

    /// Case-insensitive exact match against an available tag.
    ///
    /// A wildcard matches any tag; a tag range matches only an equal tag,
    /// case-insensitively per BCP 47.
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

    /// Whether this range names a primary language only, with no script,
    /// region or variant subtags.
    ///
    /// Guards prefix matching: RFC 4647 basic filtering has `fr` prefix-match
    /// `fr-CA`, but not `fr-CA` prefix-match `fr-CA-variant`.
    pub fn is_basic_range(&self) -> bool {
        match self {
            LanguageRange::Wildcard => true,
            LanguageRange::Tag(tag) => !tag.as_str().contains('-'),
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
    pub fn preferences(&self) -> &[QualityValue<LanguageRange>] {
        &self.preferences
    }

    /// The preferences by descending quality, ties broken by wire order.
    ///
    /// Zero-quality preferences are excluded: RFC 9110 gives `q=0` the meaning
    /// "not acceptable", so a language named that way must not be selected.
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
    /// A malformed parameter segment was found.
    #[error("malformed language preference parameter `{0}`")]
    MalformedParameter(String),
    /// A parameter other than `q` was provided.
    #[error("unsupported language preference parameter `{0}`")]
    UnsupportedParameter(String),
    /// A `q` value was syntactically invalid.
    #[error("invalid language preference quality value `{0}`")]
    InvalidQuality(String),
    /// A language tag was invalid.
    #[error("invalid language tag `{value}`")]
    InvalidLanguageTag {
        /// The original invalid input.
        value: String,
        /// The source parser error.
        #[source]
        source: langtag::InvalidLangTag<String>,
    },
}

#[cfg(test)]
mod tests {
    use crate::{
        language::{LanguageFilter, LanguageFilterError, LanguageRange},
        quality::{Quality, QualityValue},
    };
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
        assert!(LanguageRange::Wildcard.is_basic_range());
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
    fn only_a_primary_language_subtag_is_a_basic_range() {
        assert!(range("fr").is_basic_range());
        assert!(range("en").is_basic_range());
        assert!(!range("fr-CA").is_basic_range());
        assert!(!range("en-US").is_basic_range());
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
        assert!(matches!(LanguageFilter::parse("fr;q=1.1"), Err(LanguageFilterError::InvalidQuality(quality)) if quality == "1.1"));
        assert!(matches!(LanguageFilter::parse("fr;q"), Err(LanguageFilterError::MalformedParameter(_))));
    }

    #[test]
    fn a_parameter_other_than_quality_is_rejected() {
        assert!(matches!(LanguageFilter::parse("fr;level=1"), Err(LanguageFilterError::UnsupportedParameter(name)) if name == "level"));
    }

    #[test]
    fn an_invalid_language_tag_is_rejected() {
        assert!(matches!(LanguageFilter::parse("123"), Err(LanguageFilterError::InvalidLanguageTag { .. })));
    }
}
