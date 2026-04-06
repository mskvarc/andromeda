//! Language range and filter types for BCP 47 language negotiation.

use crate::http::model::quality::QualityValue;
use langtag::{LangTag, LangTagBuf};
use std::{cmp::Reverse, fmt};
use thiserror::Error;

/// A language preference: either a wildcard or a typed BCP 47 language tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageRange {
    /// Wildcard `*` matching any available language.
    Wildcard,
    /// A validated BCP 47 language tag.
    Tag(LangTagBuf),
}

impl LanguageRange {
    /// Case-insensitive exact match against an available tag.
    ///
    /// A wildcard matches any tag. A tag range matches only if it equals
    /// `available` (case-insensitive per BCP 47).
    pub fn exact_matches(&self, available: &LangTag) -> bool {
        match self {
            LanguageRange::Wildcard => true,
            LanguageRange::Tag(requested) => {
                let requested_ref: &LangTag = requested;
                requested_ref == available
            }
        }
    }

    /// RFC 4647 Section 3.3 basic filtering prefix match.
    ///
    /// Returns `true` when this range is a proper case-insensitive prefix of
    /// `available` followed by a `-` subtag separator. Does not include exact
    /// matches -- use [`exact_matches`](Self::exact_matches) for those.
    pub fn prefix_matches(&self, available: &LangTag) -> bool {
        match self {
            LanguageRange::Wildcard => false,
            LanguageRange::Tag(requested) => {
                let range_str = requested.as_str();
                let tag_str = available.as_str();
                tag_str.len() > range_str.len() && tag_str.as_bytes()[range_str.len()] == b'-' && tag_str[..range_str.len()].eq_ignore_ascii_case(range_str)
            }
        }
    }

    /// Whether this range is a basic range (primary language only, no
    /// script/region/variant subtags).
    ///
    /// Used to guard prefix matching: `fr` should prefix-match `fr-CA`,
    /// but `fr-CA` should not prefix-match `fr-CA-variant` in NGSI-LD
    /// language negotiation (clause 5.7.2.5).
    pub fn is_basic_range(&self) -> bool {
        match self {
            LanguageRange::Wildcard => true,
            LanguageRange::Tag(tag) => {
                // A basic range has only a primary language subtag.
                // If the tag string contains no `-`, it is primary-only.
                !tag.as_str().contains('-')
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

/// Parsed NGSI-LD language filter preferences.
///
/// Stores an ordered list of quality-ranked BCP 47 language ranges derived
/// from either the `lang` query parameter (NGSI-LD clause 6.3.5) or the
/// `NGSILD-Language` request header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageFilter {
    preferences: Vec<QualityValue<LanguageRange>>,
}

impl LanguageFilter {
    /// Create a new language filter from pre-parsed preferences.
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

    /// Create a language filter from a known non-empty preference list.
    ///
    /// This skips the emptiness check. Callers must guarantee the vec is non-empty.
    /// Used by `From<NgsiLdLanguage>` where the header decoder already enforces non-empty.
    pub(crate) fn from_nonempty(preferences: Vec<QualityValue<LanguageRange>>) -> LanguageFilter {
        debug_assert!(!preferences.is_empty(), "from_nonempty called with empty vec");
        LanguageFilter { preferences }
    }

    /// Return preferences ordered by descending quality, preserving
    /// insertion order for equal-ranked entries.
    ///
    /// Preferences with zero quality are excluded.
    pub fn ordered(&self) -> Vec<&QualityValue<LanguageRange>> {
        let mut preferences = self
            .preferences
            .iter()
            .enumerate()
            .filter(|(_, pref)| pref.quality().value() > 0)
            .collect::<Vec<_>>();
        preferences.sort_by_key(|(ordinal, pref)| (Reverse(pref.quality()), *ordinal));
        preferences.into_iter().map(|(_, pref)| pref).collect()
    }
}

/// Typed parse errors for the NGSI-LD language filter.
#[derive(Debug, Error)]
pub enum LanguageFilterError {
    /// The query parameter or preference list was empty.
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
    use super::*;
    use crate::http::model::quality::Quality;

    fn tag(s: &str) -> LangTagBuf {
        LangTagBuf::new(s.to_string()).expect("valid tag")
    }

    fn range_tag(s: &str) -> LanguageRange {
        LanguageRange::Tag(tag(s))
    }

    #[test]
    fn wildcard_exact_matches_any_tag() {
        assert!(LanguageRange::Wildcard.exact_matches(LangTag::new("fr").expect("valid")));
        assert!(LanguageRange::Wildcard.exact_matches(LangTag::new("en-US").expect("valid")));
    }

    #[test]
    fn exact_match_case_insensitive() {
        assert!(range_tag("fr-CA").exact_matches(LangTag::new("fr-CA").expect("valid")));
        assert!(range_tag("fr-CA").exact_matches(LangTag::new("FR-ca").expect("valid")));
    }

    #[test]
    fn exact_match_does_not_match_prefix() {
        assert!(!range_tag("fr").exact_matches(LangTag::new("fr-CA").expect("valid")));
    }

    #[test]
    fn prefix_match_fr_matches_fr_ca() {
        assert!(range_tag("fr").prefix_matches(LangTag::new("fr-CA").expect("valid")));
    }

    #[test]
    fn prefix_match_does_not_match_unrelated() {
        assert!(!range_tag("fr").prefix_matches(LangTag::new("de").expect("valid")));
    }

    #[test]
    fn prefix_match_does_not_match_partial_subtag() {
        // "fr" should not match "fra" (no `-` separator)
        assert!(!range_tag("fr").prefix_matches(LangTag::new("fra").expect("valid")));
    }

    #[test]
    fn prefix_match_wildcard_returns_false() {
        assert!(!LanguageRange::Wildcard.prefix_matches(LangTag::new("fr").expect("valid")));
    }

    #[test]
    fn is_basic_range_for_primary_only() {
        assert!(range_tag("fr").is_basic_range());
        assert!(range_tag("en").is_basic_range());
    }

    #[test]
    fn is_not_basic_range_for_tag_with_region() {
        assert!(!range_tag("fr-CA").is_basic_range());
        assert!(!range_tag("en-US").is_basic_range());
    }

    #[test]
    fn wildcard_is_basic_range() {
        assert!(LanguageRange::Wildcard.is_basic_range());
    }

    // --- LanguageFilter tests ---

    fn qv(range: LanguageRange, quality: u16) -> QualityValue<LanguageRange> {
        QualityValue::new(range, Quality::new(quality).expect("valid quality"))
    }

    #[test]
    fn ordered_sorts_by_quality_descending() {
        let filter = LanguageFilter::new(vec![
            qv(range_tag("fr-CH"), 1000),
            qv(range_tag("fr"), 900),
            qv(range_tag("en"), 800),
            qv(LanguageRange::Wildcard, 500),
        ])
        .expect("non-empty");

        let ordered = filter.ordered();
        assert_eq!(ordered[0].value().to_string(), "fr-CH");
        assert_eq!(ordered[1].value().to_string(), "fr");
        assert_eq!(ordered[2].value().to_string(), "en");
        assert_eq!(ordered[3].value().to_string(), "*");
    }

    #[test]
    fn ordered_filters_zero_quality() {
        let filter = LanguageFilter::new(vec![qv(range_tag("de"), 0), qv(range_tag("en"), 800)]).expect("non-empty");

        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].value().to_string(), "en");
    }

    #[test]
    fn ordered_preserves_insertion_order_for_equal_quality() {
        let filter = LanguageFilter::new(vec![qv(range_tag("de"), 1000), qv(range_tag("en"), 1000), qv(range_tag("fr"), 1000)]).expect("non-empty");

        let ordered = filter.ordered();
        assert_eq!(ordered[0].value().to_string(), "de");
        assert_eq!(ordered[1].value().to_string(), "en");
        assert_eq!(ordered[2].value().to_string(), "fr");
    }

    #[test]
    fn rejects_empty_preferences() {
        let result = LanguageFilter::new(vec![]);
        assert!(matches!(result, Err(LanguageFilterError::Empty)));
    }

    #[test]
    fn from_ngsild_language_header() {
        use crate::http::header::NgsiLdLanguage;
        use axum::http::HeaderValue;
        use headers::Header;

        let val = HeaderValue::from_str("fr, en;q=0.8").expect("valid header value");
        let mut iter = std::iter::once(&val);
        let header = NgsiLdLanguage::decode(&mut iter).expect("valid");
        let filter = LanguageFilter::from(header);
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].value().to_string(), "fr");
        assert_eq!(ordered[1].value().to_string(), "en");
    }
}
