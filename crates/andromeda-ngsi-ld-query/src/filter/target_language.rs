//! The target language a `q` bracket names (clause 4.9).
//!
//! Clause 4.5.18.2 keys a languageMap by "language tags as defined by IETF RFC
//! 5646", and clause 4.9 makes the target value of a `LanguageProperty` "the
//! Value associated to the matching key-value pair ... where the key matches
//! the target language". Annex C.5.13.2 writes the target language as a full
//! tag (`marque[en-GB]`), so a bracket parses as every RFC 5646
//! `Language-Tag`, and as an RFC 4647 extended range with a wildcard, which no
//! tag can spell. Which of them a request may carry is the
//! `q_target_languages` level's to say.

use crate::{
    error::QueryError,
    filter::{
        language_matching::LanguageMatching,
        language_range::{LanguageRange, extended_filter},
    },
};
use langtag::{LangTag, LangTagBuf};

/// A target language as a `q` bracket carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetLanguage {
    /// An RFC 5646 language tag: `en-GB`, `zh-Hant-TW`, `x-private`.
    Tag(LangTagBuf),
    /// An RFC 4647 extended range holding a `*` subtag: `de-*-DE`.
    Range(LanguageRange),
}

impl TargetLanguage {
    /// Read the text of a bracket as a language tag, or failing that as an
    /// extended range with a wildcard.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::TrailingMemberInvalid`] when the text is neither.
    pub fn parse(text: &str) -> Result<TargetLanguage, QueryError> {
        LangTagBuf::new(text.to_owned()).map(TargetLanguage::Tag).or_else(|tag| {
            LanguageRange::new(text.to_owned())
                .map(TargetLanguage::Range)
                .map_err(|range| QueryError::TrailingMemberInvalid { tag, range })
        })
    }

    /// The selector this target language reads a languageMap with.
    #[must_use]
    pub fn selector(&self) -> LanguageSelector<'_> {
        match self {
            TargetLanguage::Tag(tag) => LanguageSelector::Tag(tag),
            TargetLanguage::Range(range) => LanguageSelector::Range(range),
        }
    }
}

/// A target language borrowed from the path that names it, as the evaluator
/// and the store compilation read a languageMap with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSelector<'a> {
    /// A language tag.
    Tag(&'a LangTag),
    /// An extended range holding a `*` subtag.
    Range(&'a LanguageRange),
}

impl<'a> LanguageSelector<'a> {
    /// Whether the languageMap key `tag` is a key this target language
    /// selects under `matching`.
    ///
    /// Tag equality compares the two tags case-insensitively, which RFC 5646
    /// section 2.1.1 makes the only comparison a tag has. A range has no
    /// equality reading, so it filters under either mode: one only reaches a
    /// filter evaluated by tag equality when it was admitted under the
    /// `language-ranges` level, as a stored subscription's `q` is.
    #[must_use]
    pub fn admits(self, tag: &LangTag, matching: LanguageMatching) -> bool {
        match (self, matching) {
            (LanguageSelector::Tag(wanted), LanguageMatching::TagEquality) => wanted == tag,
            (LanguageSelector::Tag(_) | LanguageSelector::Range(_), LanguageMatching::ExtendedFiltering)
            | (LanguageSelector::Range(_), LanguageMatching::TagEquality) => extended_filter(self.as_str(), tag),
        }
    }

    /// The target language as written.
    #[must_use]
    pub fn as_str(self) -> &'a str {
        match self {
            LanguageSelector::Tag(tag) => tag.as_str(),
            LanguageSelector::Range(range) => range.as_str(),
        }
    }

    /// The `-`-separated subtags, `*` included.
    pub fn subtags(self) -> impl Iterator<Item = &'a str> {
        self.as_str().split('-')
    }

    /// Whether this is an extended range, which only the `language-ranges`
    /// level of the `q_target_languages` extension admits.
    #[must_use]
    pub const fn is_range(self) -> bool {
        matches!(self, LanguageSelector::Range(_))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{
            language_matching::LanguageMatching,
            target_language::{LanguageSelector, TargetLanguage},
        },
    };
    use langtag::LangTag;

    fn parsed(text: &str) -> TargetLanguage {
        TargetLanguage::parse(text).expect("a target language")
    }

    fn tag(text: &str) -> &LangTag {
        LangTag::new(text).expect("a valid tag")
    }

    #[test]
    fn every_rfc_5646_tag_form_parses_as_a_tag() {
        for text in [
            "en",
            "en-GB",
            "zh-Hant-TW",
            "es-419",
            "de-CH-1901",
            "x-private",
            "i-klingon",
            "en-US-u-ca-gregory",
        ] {
            assert!(matches!(parsed(text), TargetLanguage::Tag(_)), "{text}");
        }
    }

    #[test]
    fn a_wildcard_range_parses_as_a_range() {
        for text in ["de-*-DE", "*-GB", "en-*"] {
            assert!(matches!(parsed(text), TargetLanguage::Range(_)), "{text}");
        }
    }

    #[test]
    fn a_text_neither_tag_nor_range_is_refused() {
        for text in ["en--GB", "toolongsubtag9", "-en", "e*", "en_GB-x", "*"] {
            let error = TargetLanguage::parse(text).expect_err(text);
            assert!(matches!(error, QueryError::TrailingMemberInvalid { .. }), "{text}: {error:?}");
        }
    }

    /// RFC 5646 section 2.1.1: case carries no distinction.
    #[test]
    fn tag_equality_ignores_case_and_nothing_else() {
        let lower = parsed("en-gb");
        assert!(lower.selector().admits(tag("en-GB"), LanguageMatching::TagEquality));
        let english = parsed("en");
        assert!(!english.selector().admits(tag("en-GB"), LanguageMatching::TagEquality));
        assert!(english.selector().admits(tag("EN"), LanguageMatching::TagEquality));
    }

    #[test]
    fn extended_filtering_reads_a_tag_as_a_range() {
        let english = parsed("en");
        assert!(english.selector().admits(tag("en-GB"), LanguageMatching::ExtendedFiltering));
        assert!(!english.selector().admits(tag("fr"), LanguageMatching::ExtendedFiltering));
    }

    /// A range has no equality reading, so it filters even under tag equality.
    #[test]
    fn a_range_filters_under_either_mode() {
        let range = parsed("de-*-DE");
        for matching in [LanguageMatching::TagEquality, LanguageMatching::ExtendedFiltering] {
            assert!(range.selector().admits(tag("de-Latn-DE"), matching), "{matching:?}");
            assert!(!range.selector().admits(tag("de-Deva"), matching), "{matching:?}");
        }
    }

    #[test]
    fn the_selector_spells_its_subtags() {
        let range = parsed("de-*-DE");
        assert!(range.selector().is_range());
        pretty_assertions::assert_eq!(range.selector().subtags().collect::<Vec<_>>(), vec!["de", "*", "DE"]);
        assert!(!matches!(parsed("en").selector(), LanguageSelector::Range(_)));
    }
}
