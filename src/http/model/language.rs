//! Language range types for BCP 47 language negotiation.

use langtag::{LangTag, LangTagBuf};
use std::fmt;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
