//! A string the supplied `@context` coerces to a URI.

use crate::identifier_length::IdentifierTooLong;
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, Into};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// Why a string cannot be a [`CoercedTerm`].
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CoercedTermError {
    /// The value was empty.
    #[error("a context-coerced value must not be empty")]
    Empty,
    /// The value is longer than an identifier may be.
    #[error(transparent)]
    TooLong(#[from] IdentifierTooLong),
    /// The value holds whitespace, which neither a term nor an IRI can.
    #[error("a context-coerced value must not contain whitespace")]
    Whitespace,
}

/// One value of a member the specification types as a string that "shall be
/// type coerced to a URI based on the supplied @context", as it is written on
/// the wire.
///
/// Those members are `vocab` (clause 5.2.35, Table 5.2.35-1), `valueType`
/// (clauses 4.5.2.2 and 4.5.2.3: "a string value which shall be type coerced
/// into a datatype URI"; Tables 5.2.5-1, 5.2.7-1, 5.2.35-1 and 5.2.36-1: "A
/// String Value which shall be type coerced to a URI based on the supplied
/// @context") and `objectType` (Tables 5.2.6-1 and 5.2.37-1: "Both short hand
/// string(s) (type name) or URI(s) are allowed"). None restricts the string further, so the value is whatever the
/// `@context` can expand: a term such as `non-commercial` (clause C.5.2), a
/// compact IRI or an absolute IRI. That is wider than the clause 4.6.2 name
/// grammar, whose short names have no `-`, so the type guards only what can
/// never be a JSON-LD term or an IRI: an empty value, one holding whitespace,
/// and one past the identifier length bound.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, Into)]
#[serde(try_from = "String", into = "String")]
#[as_ref(str, CompactString)]
#[display("{_0}")]
#[into(String)]
pub struct CoercedTerm(CompactString);

impl CoercedTerm {
    /// Borrow the value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// Validating parser: no derive expresses a fallible parse.
impl FromStr for CoercedTerm {
    type Err = CoercedTermError;

    fn from_str(value: &str) -> Result<CoercedTerm, CoercedTermError> {
        if value.is_empty() {
            return Err(CoercedTermError::Empty);
        }
        IdentifierTooLong::check(value)?;
        if value.chars().any(char::is_whitespace) {
            return Err(CoercedTermError::Whitespace);
        }
        Ok(CoercedTerm(CompactString::from(value)))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for CoercedTerm {
    type Error = CoercedTermError;

    fn try_from(value: String) -> Result<CoercedTerm, CoercedTermError> {
        value.parse()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        coerced_term::{CoercedTerm, CoercedTermError},
        identifier_length::IDENTIFIER_BYTE_LIMIT,
    };

    #[test]
    fn a_hyphenated_term_a_compact_iri_and_an_absolute_iri_are_accepted() {
        for accepted in ["non-commercial", "schema:Thing", "https://example.org/vocab/non-commercial"] {
            let term: CoercedTerm = accepted.parse().expect("an accepted coerced value");
            pretty_assertions::assert_eq!(term.as_str(), accepted);
        }
    }

    #[test]
    fn an_empty_value_is_refused() {
        pretty_assertions::assert_eq!("".parse::<CoercedTerm>(), Err(CoercedTermError::Empty));
    }

    #[test]
    fn a_value_holding_whitespace_is_refused() {
        for refused in [" ", "non commercial", "tab\tseparated", "trailing "] {
            pretty_assertions::assert_eq!(refused.parse::<CoercedTerm>(), Err(CoercedTermError::Whitespace), "{refused:?}");
        }
    }

    #[test]
    fn a_value_past_the_identifier_bound_is_refused_and_one_at_it_is_not() {
        assert!("a".repeat(IDENTIFIER_BYTE_LIMIT).parse::<CoercedTerm>().is_ok());
        let refused = "a"
            .repeat(IDENTIFIER_BYTE_LIMIT + 1)
            .parse::<CoercedTerm>()
            .expect_err("an over-long coerced value");
        assert!(matches!(refused, CoercedTermError::TooLong(_)), "{refused:?}");
    }

    #[test]
    fn serde_round_trips_a_term_verbatim_and_refuses_an_empty_string() {
        let term: CoercedTerm = serde_json::from_str("\"non-commercial\"").expect("a coerced term");
        pretty_assertions::assert_eq!(serde_json::to_string(&term).expect("serialises"), "\"non-commercial\"");
        assert!(serde_json::from_str::<CoercedTerm>("\"\"").is_err());
    }
}
