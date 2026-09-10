//! ICU collation identifiers for `orderBy` string sorting (clause 4.23).

use langtag::LangTagBuf;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// ICU collation identifier for `orderBy` string sorting (NGSI-LD S4.23).
///
/// Holds the BCP47 tag it parsed rather than the text it parsed from: a
/// collation identifier is a language tag, optionally carrying Unicode
/// collation settings in a `-u-` extension (IETF RFC 6067), and keeping the
/// parsed form is what stops the tag being re-derived wherever the identifier
/// is used.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IcuCollation(LangTagBuf);

impl IcuCollation {
    /// The ICU "root" collation identifier.
    ///
    /// Clause 4.23.1: "for strings the default collation order shall be defined
    /// as using ICU \"root\" collation order (\"und-x-icu\")". A query that
    /// names no collation sorts strings under this one, so the default is
    /// stated here beside the type that carries a client-supplied alternative.
    pub const ROOT: &'static str = "und-x-icu";

    /// Parses and validates an ICU collation identifier.
    ///
    /// A bare language tag names an ICU collation — clause 4.23.1's own default
    /// `und-x-icu` is one, and it carries no `-u-` extension at all — so no
    /// extension is required. When one is present, every keyword key it
    /// declares has to be a collation setting: clause 5.7.2.4 raises
    /// `BadRequestData` for "a preferred collation setting [that] does not
    /// conform to a valid ICU collation", and a tag whose keywords select a
    /// numbering system or a calendar states no collation preference at all.
    ///
    /// # Errors
    ///
    /// Returns [`IcuCollationError::Empty`] when `raw` is blank,
    /// [`IcuCollationError::InvalidTag`] when it is not a BCP47 language tag,
    /// and [`IcuCollationError::UnknownCollationKeyword`] when its Unicode
    /// extension declares a keyword RFC 6067 does not define for collation.
    pub fn parse(raw: &str) -> Result<IcuCollation, IcuCollationError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(IcuCollationError::Empty);
        }

        let tag = LangTagBuf::new(trimmed.to_string()).map_err(|source| IcuCollationError::InvalidTag {
            value: trimmed.to_string(),
            source,
        })?;

        match unicode_keyword_that_is_not_a_collation_setting(trimmed) {
            Some(keyword) => Err(IcuCollationError::UnknownCollationKeyword {
                value: trimmed.to_string(),
                keyword: keyword.to_string(),
            }),
            None => Ok(IcuCollation(tag)),
        }
    }

    /// The collation identifier (a BCP47 language tag).
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for IcuCollation {
    type Error = IcuCollationError;

    fn try_from(value: String) -> Result<IcuCollation, IcuCollationError> {
        IcuCollation::parse(&value)
    }
}

impl From<IcuCollation> for String {
    fn from(value: IcuCollation) -> String {
        value.0.into_string()
    }
}

/// The BCP47 singleton that introduces the Unicode locale extension, `u`.
///
/// IETF RFC 6067 defines this extension, and clause 4.23.3 spells its examples
/// (`und-u-ks-identic`, `de-u-co-phonebk`) with it.
const UNICODE_EXTENSION_SINGLETON: &str = "u";

/// The length of a BCP47 singleton subtag, which introduces an extension.
const SINGLETON_LENGTH: usize = 1;

/// The subtag length that distinguishes a Unicode keyword *key* from a value.
///
/// RFC 6067 fixes keys at two characters and keyword values and attributes at
/// three or more, so the length alone decides which of the two a subtag is.
const KEYWORD_KEY_LENGTH: usize = 2;

/// The first Unicode keyword key in `tag` that names no collation setting.
///
/// The already-validated tag text is walked subtag by subtag rather than
/// through the language-tag crate's extension accessor, which indexes one byte
/// past the end of any tag that carries no extension at all — `de-DE` among
/// them — and so cannot be asked whether an extension is present.
///
/// A singleton opens an extension and closes the previous one, so the keys are
/// exactly the two-character subtags between the `u` singleton and the next
/// singleton. Only those are examined: a two-character subtag anywhere else in
/// the tag is a language or a region, not a keyword.
fn unicode_keyword_that_is_not_a_collation_setting(tag: &str) -> Option<&str> {
    let mut in_unicode_extension = false;
    for subtag in tag.split('-') {
        if subtag.len() == SINGLETON_LENGTH {
            in_unicode_extension = subtag.eq_ignore_ascii_case(UNICODE_EXTENSION_SINGLETON);
        } else if in_unicode_extension && subtag.len() == KEYWORD_KEY_LENGTH && !is_collation_keyword_key(subtag) {
            return Some(subtag);
        }
    }
    None
}

/// The Unicode keyword keys that carry a collation setting.
///
/// The collation keys of the Unicode BCP47 extension registry RFC 6067 governs:
/// collation type, alternate handling, backward second level, case level, case
/// first, hiragana quaternary, normalisation, numeric ordering, reordering,
/// strength, maximum variable and variable top.
const COLLATION_KEYWORD_KEYS: [&str; 12] = ["co", "ka", "kb", "kc", "kf", "kh", "kk", "kn", "kr", "ks", "kv", "vt"];

/// Whether `key` names a collation setting.
///
/// BCP47 subtags are case-insensitive, so a client that spells its keyword key
/// in upper case names the same setting.
fn is_collation_keyword_key(key: &str) -> bool {
    COLLATION_KEYWORD_KEYS.iter().any(|collation_key| key.eq_ignore_ascii_case(collation_key))
}

/// Reasons an ICU collation identifier is rejected.
#[derive(Debug, Error)]
pub enum IcuCollationError {
    /// The identifier was empty or whitespace only.
    #[error("collation identifier must not be empty")]
    Empty,
    /// The identifier's Unicode extension declared a keyword that is not a
    /// collation setting.
    #[error("collation identifier '{value}' declares '{keyword}', which is not an ICU collation setting")]
    UnknownCollationKeyword {
        /// The rejected value.
        value: String,
        /// The keyword key that names no collation setting.
        keyword: String,
    },
    /// The identifier was not a well-formed BCP47 language tag.
    #[error("invalid BCP47 collation identifier '{value}'")]
    InvalidTag {
        /// The rejected value.
        value: String,
        /// The underlying language-tag parse error.
        #[source]
        source: langtag::InvalidLangTag<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collation_identifier_keeps_the_tag_it_parsed() {
        let collation = IcuCollation::parse("und-u-ks-identic").expect("valid identifier");
        assert_eq!(collation.as_str(), "und-u-ks-identic");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(IcuCollation::parse("  und-u-ks-identic  ").expect("valid").as_str(), "und-u-ks-identic");
    }

    #[test]
    fn an_empty_identifier_is_rejected() {
        assert!(matches!(IcuCollation::parse("   "), Err(IcuCollationError::Empty)));
    }

    /// Clause 4.23.1 names `und-x-icu` as the default collation order, so the
    /// identifier the specification itself spells has to parse.
    #[test]
    fn the_icu_root_collation_named_by_the_specification_is_accepted() {
        let root = IcuCollation::parse(IcuCollation::ROOT).expect("the clause 4.23.1 default must parse");
        assert_eq!(root.as_str(), "und-x-icu");
    }

    /// Clause 4.23.3 EXAMPLE 6 and EXAMPLE 7 are the two collations the
    /// specification demonstrates the parameter with.
    #[test]
    fn both_collations_the_specification_gives_as_examples_are_accepted() {
        for example in ["und-u-ks-identic", "de-u-co-phonebk"] {
            assert_eq!(IcuCollation::parse(example).expect("a spec example must parse").as_str(), example);
        }
    }

    /// A language tag with no Unicode extension still names an ICU collation:
    /// the locale's own default collation order.
    #[test]
    fn a_bare_language_tag_names_the_locale_default_collation() {
        assert_eq!(IcuCollation::parse("de-DE").expect("a bare tag names a collation").as_str(), "de-DE");
    }

    #[test]
    fn a_value_that_is_not_a_language_tag_is_rejected() {
        let error = IcuCollation::parse("not a tag").expect_err("must reject");
        assert!(matches!(error, IcuCollationError::InvalidTag { .. }), "got {error:?}");
    }

    /// Clause 5.7.2.4 raises `BadRequestData` for a preferred collation setting
    /// that is not a valid ICU collation; a keyword selecting a numbering
    /// system or a calendar expresses no collation preference.
    #[test]
    fn a_unicode_keyword_that_is_not_a_collation_setting_is_rejected() {
        for rejected in ["de-u-nu-latn", "de-u-ca-buddhist", "und-u-zz-nonsense"] {
            let error = IcuCollation::parse(rejected).expect_err("must reject");
            assert!(matches!(error, IcuCollationError::UnknownCollationKeyword { .. }), "{rejected}: got {error:?}");
        }
    }

    /// BCP47 subtags are case-insensitive, so the keyword check cannot depend
    /// on the casing a client happened to send.
    #[test]
    fn a_collation_keyword_is_recognised_whatever_case_it_is_written_in() {
        assert!(IcuCollation::parse("de-U-CO-phonebk").is_ok());
    }
}
