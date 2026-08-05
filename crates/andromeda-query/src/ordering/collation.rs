//! ICU collation identifiers for `orderBy` string sorting (clause 4.23).

use langtag::LangTagBuf;
use thiserror::Error;

/// ICU collation identifier for `orderBy` string sorting (NGSI-LD S4.23).
///
/// Holds the BCP47 tag it parsed rather than the text it parsed from: a
/// collation identifier is a language tag carrying a Unicode `-u-` extension,
/// and keeping the parsed form is what stops the tag being re-derived wherever
/// the identifier is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcuCollation(LangTagBuf);

impl IcuCollation {
    /// Parses and validates an ICU collation identifier.
    pub fn parse(raw: &str) -> Result<IcuCollation, IcuCollationError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(IcuCollationError::Empty);
        }

        let tag = LangTagBuf::new(trimmed.to_string()).map_err(|source| IcuCollationError::InvalidTag {
            value: trimmed.to_string(),
            source,
        })?;

        if !tag.as_str().contains(UNICODE_EXTENSION_MARKER) {
            return Err(IcuCollationError::MissingUnicodeExtension(trimmed.to_string()));
        }

        Ok(IcuCollation(tag))
    }

    /// The identifier as PostgreSQL names the collation.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// The subtag separator that introduces a BCP47 Unicode extension.
///
/// Clause 4.23 requires an ICU collation identifier, which is a language tag
/// carrying collation settings in this extension; a bare language tag names no
/// collation.
const UNICODE_EXTENSION_MARKER: &str = "-u-";

/// Reasons an ICU collation identifier is rejected.
#[derive(Debug, Error)]
pub enum IcuCollationError {
    /// The identifier was empty or whitespace only.
    #[error("collation identifier must not be empty")]
    Empty,
    /// The identifier parsed as BCP47 but carried no `-u-` extension.
    #[error("collation identifier must contain Unicode extension subtags: {0}")]
    MissingUnicodeExtension(String),
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

    #[test]
    fn a_tag_without_a_unicode_extension_names_no_collation() {
        // Clause 4.23: the collation settings live in the `-u-` extension.
        let error = IcuCollation::parse("de-DE").expect_err("must reject");
        assert!(matches!(error, IcuCollationError::MissingUnicodeExtension(_)), "got {error:?}");
    }

    #[test]
    fn a_value_that_is_not_a_language_tag_is_rejected() {
        let error = IcuCollation::parse("not a tag").expect_err("must reject");
        assert!(matches!(error, IcuCollationError::InvalidTag { .. }), "got {error:?}");
    }
}
