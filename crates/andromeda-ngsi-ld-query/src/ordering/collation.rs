//! ICU collation identifiers for `orderBy` string sorting (clause 4.23).

use crate::ordering::unicode_collation_keyword::{CollationKeyword, UnregisteredKeyword, unregistered_collation_keyword};
use derive_more::Display;
use langtag::{InvalidLangTag, LangTagBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// ICU collation identifier for `orderBy` string sorting (NGSI-LD S4.23).
///
/// Holds the BCP47 tag it parsed rather than the text it parsed from: a
/// collation identifier is a language tag, optionally carrying Unicode
/// collation settings in a `-u-` extension (IETF RFC 6067), and keeping the
/// parsed form is what stops the tag being re-derived wherever the identifier
/// is used.
#[derive(Debug, Clone, Display, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[display("{}", _0.as_str())]
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
    /// Clauses 5.7.2.4 and 5.7.4.4 raise `BadRequestData` for "a preferred
    /// collation setting [that] does not conform to a valid ICU collation (see
    /// IETF RFC 6067)". Two things are decided here and one deliberately is
    /// not.
    ///
    /// The tag has to be a well-formed BCP47 language tag, because an ICU
    /// collation is named by one — clause 4.23.1's own default `und-x-icu` is
    /// one, and it carries no `-u-` extension at all, so no extension is
    /// required.
    ///
    /// Where an extension is present, every keyword it declares has to be one
    /// RFC 6067 registers for collation: both the key, because a keyword
    /// selecting a numbering system or a calendar states no collation
    /// preference, and the type, because ICU accepts an unregistered collation
    /// type and silently substitutes the key's default for it — a request
    /// naming `de-u-co-nonsense` would otherwise be answered, successfully, in
    /// an order it never asked for.
    ///
    /// Whether the locale itself names a collation ICU holds is not decided
    /// here. ICU's set of known languages is its own — it excludes ISO 639-2
    /// codes such as `mul` and `zxx` that any embedded language table would
    /// admit — so a copy of it kept in this process would refuse tags ICU
    /// accepts and accept tags it does not. The store that provisions the
    /// collation asks ICU instead, under a validation level that refuses an
    /// unknown locale rather than silently creating a root-sorting object, and
    /// classifies that refusal as this same `BadRequestData`.
    ///
    /// # Errors
    ///
    /// Returns [`IcuCollationError::Empty`] when `raw` is blank,
    /// [`IcuCollationError::InvalidTag`] when it is not a BCP47 language tag,
    /// [`IcuCollationError::UnknownCollationKeyword`] when its Unicode
    /// extension declares a keyword key RFC 6067 does not define for collation,
    /// and [`IcuCollationError::UnknownCollationKeywordType`] when a collation
    /// key carries a type the registry does not list for it.
    pub fn parse(raw: &str) -> Result<IcuCollation, IcuCollationError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(IcuCollationError::Empty);
        }

        let tag = LangTagBuf::new(trimmed.to_string()).map_err(IcuCollationError::InvalidTag)?;

        match unregistered_collation_keyword(trimmed) {
            Some(UnregisteredKeyword::Key(_)) => Err(IcuCollationError::UnknownCollationKeyword),
            Some(UnregisteredKeyword::Type { key, value: _ }) => Err(IcuCollationError::UnknownCollationKeywordType { keyword: key }),
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

/// Reasons an ICU collation identifier is rejected.
#[derive(Debug, Error)]
pub enum IcuCollationError {
    /// The identifier was empty or whitespace only.
    #[error("collation identifier must not be empty")]
    Empty,
    /// The identifier's Unicode extension declared a keyword that is not a
    /// collation setting.
    #[error("collation identifier declares a Unicode keyword that is not an ICU collation setting")]
    UnknownCollationKeyword,
    /// A collation keyword carried a type IETF RFC 6067 does not register for
    /// that key.
    #[error("collation identifier sets '{keyword}' to a type IETF RFC 6067 does not register for it")]
    UnknownCollationKeywordType {
        /// The collation key the unregistered type was written under.
        keyword: CollationKeyword,
    },
    /// The identifier was not a well-formed BCP47 language tag.
    #[error("invalid BCP47 collation identifier")]
    InvalidTag(#[source] InvalidLangTag<String>),
}

#[cfg(test)]
mod tests {
    use crate::ordering::{
        collation::{IcuCollation, IcuCollationError},
        unicode_collation_keyword::CollationKeyword,
    };
    use pretty_assertions::assert_eq;

    /// The identifier renders as the tag it parsed, which is what an error
    /// message and a `CREATE COLLATION` both name it by.
    #[test]
    fn a_collation_identifier_displays_as_its_tag() {
        assert_eq!(IcuCollation::parse("de-u-co-phonebk").expect("valid identifier").to_string(), "de-u-co-phonebk");
    }

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
        assert!(matches!(error, IcuCollationError::InvalidTag(_)), "got {error:?}");
    }

    /// Clause 5.7.2.4 raises `BadRequestData` for a preferred collation setting
    /// that is not a valid ICU collation; a keyword selecting a numbering
    /// system or a calendar expresses no collation preference.
    #[test]
    fn a_unicode_keyword_that_is_not_a_collation_setting_is_rejected() {
        for rejected in ["de-u-nu-latn", "de-u-ca-buddhist", "und-u-zz-nonsense"] {
            let error = IcuCollation::parse(rejected).expect_err("must reject");
            assert!(matches!(error, IcuCollationError::UnknownCollationKeyword), "{rejected}: got {error:?}");
        }
    }

    /// IETF RFC 6067 registers the types as well as the keys. ICU accepts an
    /// unregistered collation type and silently sorts by the key's default, so
    /// clause 5.7.4.4's `BadRequestData` has to be raised here rather than
    /// waiting for a refusal that never comes.
    #[test]
    fn a_collation_keyword_carrying_an_unregistered_type_is_rejected() {
        for rejected in ["de-u-co-nonsense", "de-u-ks-zzz", "de-u-ka-bogus", "de-u-kf-true"] {
            let error = IcuCollation::parse(rejected).expect_err("must reject");
            assert!(
                matches!(error, IcuCollationError::UnknownCollationKeywordType { .. }),
                "{rejected}: got {error:?}"
            );
        }
    }

    /// BCP47 subtags are case-insensitive, so the keyword check cannot depend
    /// on the casing a client happened to send.
    #[test]
    fn a_collation_keyword_is_recognised_whatever_case_it_is_written_in() {
        assert!(IcuCollation::parse("de-U-CO-phonebk").is_ok());
    }

    /// The refusal names the registered key the unregistered type was written
    /// under, which is what the client has to correct.
    #[test]
    fn an_unregistered_type_is_reported_under_its_collation_key() {
        let error = IcuCollation::parse("de-u-ks-zzz").expect_err("must reject");
        assert!(
            matches!(error, IcuCollationError::UnknownCollationKeywordType { keyword: CollationKeyword::Ks }),
            "got {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "collation identifier sets 'ks' to a type IETF RFC 6067 does not register for it"
        );
    }
}
