//! RFC 4647 extended language ranges and the extended filtering that matches
//! them against language tags.
//!
//! The `language-ranges` level of the `q_target_languages` extension reads
//! every target language of a `q` bracket as an RFC 4647 section 2.2 extended language range. A range
//! without a `*` subtag is spelled exactly like the RFC 5646 tag it names, so
//! [`LanguageRange`] holds only the ranges no tag can spell, and
//! [`extended_filter`] takes a range by its text so that a tag read as a range
//! filters the same way.

use langtag::LangTag;
use thiserror::Error;

/// The wildcard subtag of an extended language range (RFC 4647 section 2.2).
const WILDCARD: &str = "*";

/// The longest subtag an extended language range admits: `1*8ALPHA` and
/// `1*8alphanum` in RFC 4647 section 2.2.
const MAX_SUBTAG_LENGTH: usize = 8;

/// An RFC 4647 section 2.2 extended language range holding at least one `*`
/// subtag: `de-*-DE`, `*-GB`, `en-*`.
///
/// The grammar is `(1*8ALPHA / "*") *("-" (1*8alphanum / "*"))`. A `*` alone
/// is not one: clause 4.9 gives `[*]` to the every-language marker.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
#[display("{_0}")]
pub struct LanguageRange(String);

/// Why a text is not an extended language range with a wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LanguageRangeError {
    /// The first subtag is neither 1 to 8 letters nor `*`.
    #[error("the first subtag of a language range is 1 to 8 letters or '*'")]
    FirstSubtag,
    /// A later subtag is neither 1 to 8 letters and digits nor `*`.
    #[error("subtag {index} of a language range is neither 1 to 8 letters and digits nor '*'")]
    Subtag {
        /// One-based position of the subtag.
        index: usize,
    },
    /// No subtag is `*`, so the text is a language tag or nothing.
    #[error("a language range without a '*' subtag is a language tag")]
    NoWildcard,
    /// The text is `*` alone, the clause 4.9 every-language marker.
    #[error("'*' alone selects every language, it is not a language range")]
    LoneWildcard,
}

impl LanguageRange {
    /// Validate `text` as an extended language range holding a wildcard.
    ///
    /// # Errors
    ///
    /// Returns the [`LanguageRangeError`] naming the RFC 4647 section 2.2 rule
    /// the text breaks, [`LanguageRangeError::NoWildcard`] for a text with no
    /// `*` subtag and [`LanguageRangeError::LoneWildcard`] for `*` alone.
    pub fn new(text: String) -> Result<LanguageRange, LanguageRangeError> {
        let mut subtags = text.split('-');
        if !subtags
            .next()
            .is_some_and(|first| first == WILDCARD || is_subtag(first, u8::is_ascii_alphabetic))
        {
            return Err(LanguageRangeError::FirstSubtag);
        }
        if let Some(index) = subtags.position(|subtag| subtag != WILDCARD && !is_subtag(subtag, u8::is_ascii_alphanumeric)) {
            return Err(LanguageRangeError::Subtag { index: index + 2 });
        }
        if text == WILDCARD {
            return Err(LanguageRangeError::LoneWildcard);
        }
        if !text.split('-').any(|subtag| subtag == WILDCARD) {
            return Err(LanguageRangeError::NoWildcard);
        }
        Ok(LanguageRange(text))
    }

    /// The range as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether `subtag` is 1 to 8 characters each satisfying `admitted`.
fn is_subtag(subtag: &str, admitted: fn(&u8) -> bool) -> bool {
    (1..=MAX_SUBTAG_LENGTH).contains(&subtag.len()) && subtag.bytes().all(|byte| admitted(&byte))
}

/// Whether the extended language range spelled `range` matches `tag` under
/// RFC 4647 section 3.3.2 extended filtering.
///
/// The first subtags match or the range's is `*`; then each later range
/// subtag that is not `*` is looked for further along the tag, skipping tag
/// subtags until a singleton, which no range subtag reaches past. Subtags
/// compare case-insensitively (RFC 4647 section 2).
#[must_use]
pub fn extended_filter(range: &str, tag: &LangTag) -> bool {
    let mut range_subtags = range.split('-');
    let mut tag_subtags = tag.as_str().split('-');
    let first_matches = match (range_subtags.next(), tag_subtags.next()) {
        (Some(range_first), Some(tag_first)) => range_first == WILDCARD || range_first.eq_ignore_ascii_case(tag_first),
        (None, _) | (_, None) => false,
    };
    first_matches
        && range_subtags
            .filter(|subtag| *subtag != WILDCARD)
            .all(|wanted| tag_subtags.by_ref().find_map(|held| next_step(wanted, held)).unwrap_or(false))
}

/// The RFC 4647 section 3.3.2 step one tag subtag `held` takes against the
/// range subtag `wanted`: `Some(true)` when it matches, `Some(false)` when it
/// is a singleton the match fails at, and `None` to skip it.
fn next_step(wanted: &str, held: &str) -> Option<bool> {
    if wanted.eq_ignore_ascii_case(held) {
        return Some(true);
    }
    (held.len() == 1).then_some(false)
}

#[cfg(test)]
mod tests {
    use crate::filter::language_range::{LanguageRange, LanguageRangeError, extended_filter};
    use langtag::LangTag;
    use pretty_assertions::assert_eq;

    fn admits(range: &str, tag: &str) -> bool {
        extended_filter(range, LangTag::new(tag).expect("a valid tag"))
    }

    #[test]
    fn a_wildcard_range_parses_in_every_position() {
        for range in ["de-*-DE", "*-GB", "en-*", "*-*", "de-*-DE-1996"] {
            assert_eq!(
                LanguageRange::new(range.to_owned()).map(|parsed| parsed.to_string()),
                Ok(range.to_owned()),
                "{range}"
            );
        }
    }

    #[test]
    fn a_text_outside_the_extended_range_grammar_is_refused() {
        assert_eq!(LanguageRange::new("e*".to_owned()), Err(LanguageRangeError::FirstSubtag));
        assert_eq!(LanguageRange::new("de1-*".to_owned()), Err(LanguageRangeError::FirstSubtag));
        assert_eq!(LanguageRange::new("toolongsub-*".to_owned()), Err(LanguageRangeError::FirstSubtag));
        assert_eq!(LanguageRange::new("de-*-".to_owned()), Err(LanguageRangeError::Subtag { index: 3 }));
        assert_eq!(LanguageRange::new("de--*".to_owned()), Err(LanguageRangeError::Subtag { index: 2 }));
        assert_eq!(LanguageRange::new("de-*x".to_owned()), Err(LanguageRangeError::Subtag { index: 2 }));
        assert_eq!(LanguageRange::new("en-GB".to_owned()), Err(LanguageRangeError::NoWildcard));
        assert_eq!(LanguageRange::new("*".to_owned()), Err(LanguageRangeError::LoneWildcard));
    }

    /// RFC 4647 section 3.3.2: `de-*-DE` against the tags the section lists.
    #[test]
    fn extended_filtering_follows_the_rfc_4647_examples() {
        for tag in ["de-DE", "de-de", "de-Latn-DE", "de-Latf-DE", "de-DE-x-goethe", "de-Latn-DE-1996", "de-Deva-DE"] {
            assert!(admits("de-*-DE", tag), "de-*-DE admits {tag}");
        }
        for tag in ["de", "de-x-DE", "de-Deva"] {
            assert!(!admits("de-*-DE", tag), "de-*-DE refuses {tag}");
        }
    }

    #[test]
    fn a_wildcard_matches_any_subtag_and_a_trailing_wildcard_matches_none() {
        assert!(admits("*-GB", "en-GB"));
        assert!(!admits("*-GB", "en-US"));
        assert!(admits("en-*", "en"));
        assert!(admits("en-*", "en-GB"));
        assert!(!admits("en-*", "fr"));
    }

    /// A range without a wildcard is a language tag read as a range: it
    /// matches the tags it is a prefix of, subtag by subtag.
    #[test]
    fn a_tag_read_as_a_range_matches_the_tags_it_prefixes() {
        assert!(admits("en", "en"));
        assert!(admits("en", "en-GB"));
        assert!(admits("EN", "en-gb"));
        assert!(!admits("en-GB", "en"));
        assert!(!admits("en", "eng"));
        assert!(admits("de-DE", "de-Latn-DE"));
    }
}
