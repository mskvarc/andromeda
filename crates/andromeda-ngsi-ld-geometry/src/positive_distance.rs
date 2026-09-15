//! The `PositiveNumber` distance a `near` relationship carries (clause 4.10).

use serde_json::Number;
use std::{fmt, str::FromStr};
use thiserror::Error;

/// Failures parsing a clause 4.10 `PositiveNumber` distance.
///
/// Clause 4.10's grammar spells a `near` distance as a `PositiveNumber`, an
/// IETF RFC 8259 JSON `Number` that is strictly positive. Rust's `f64` parser is
/// laxer than that grammar: it accepts a leading `+`, a bare leading or trailing
/// `.`, leading zeros, and the non-finite spellings `NaN` / `inf`. Each is
/// rejected here so only a well-formed positive distance is representable.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum PositiveDistanceParseError {
    /// The value is not an RFC 8259 JSON `Number` (a leading `+`, a bare leading
    /// or trailing `.`, a leading zero, thousands separators, or non-numeric
    /// text all land here).
    #[error("invalid distance: {0}")]
    InvalidSyntax(String),
    /// The value parsed as a JSON number but is not representable as a finite
    /// `f64` (`NaN` or an infinity). Left unchecked such a value reaches `PostGIS`
    /// as a bare identifier rather than a distance.
    #[error("non-finite distance: {0}")]
    NonFinite(String),
    /// The value is a finite number that is zero or negative. Clause 4.10's
    /// grammar is `distance equal PositiveNumber`, so a non-positive distance is
    /// not a `near` query at all.
    #[error("non-positive distance: {0}")]
    NonPositive(String),
}

/// A strictly positive distance in metres, validated against the clause 4.10
/// `PositiveNumber` grammar.
///
/// Construction goes through [`FromStr`], which enforces the RFC 8259 `Number`
/// production before checking finiteness and positivity. [`fmt::Display`] prints
/// the underlying `f64` in its canonical Rust spelling, which round-trips every
/// value this type can hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositiveDistance(f64);

impl PositiveDistance {
    /// The distance in metres. Always finite and strictly positive.
    #[must_use]
    pub const fn metres(self) -> f64 {
        self.0
    }
}

impl FromStr for PositiveDistance {
    type Err = PositiveDistanceParseError;

    fn from_str(value: &str) -> Result<PositiveDistance, PositiveDistanceParseError> {
        // `serde_json::Number` accepts exactly the RFC 8259 `Number` grammar, so
        // it rejects the leading `+`, bare `.`, and leading zeros that `f64`'s
        // own parser tolerates but clause 4.10 forbids.
        let number = Number::from_str(value).map_err(|_| PositiveDistanceParseError::InvalidSyntax(value.to_string()))?;
        let metres = number.as_f64().ok_or_else(|| PositiveDistanceParseError::NonFinite(value.to_string()))?;
        if !metres.is_finite() {
            return Err(PositiveDistanceParseError::NonFinite(value.to_string()));
        }
        if metres <= 0.0 {
            return Err(PositiveDistanceParseError::NonPositive(value.to_string()));
        }
        Ok(PositiveDistance(metres))
    }
}

impl fmt::Display for PositiveDistance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::positive_distance::{PositiveDistance, PositiveDistanceParseError};
    use std::str::FromStr;

    #[test]
    fn a_well_formed_positive_number_is_accepted_and_round_trips() {
        let cases: [(&str, f64, &str); 5] = [
            ("500", 500.0, "500"),
            ("500.0", 500.0, "500"),
            ("0.5", 0.5, "0.5"),
            ("1e3", 1000.0, "1000"),
            ("50", 50.0, "50"),
        ];
        for (input, expected_metres, canonical) in cases {
            let parsed = PositiveDistance::from_str(input).expect(input);
            // Bit-exact: the inputs are all exactly representable, and `f64`
            // equality is the property under test, not an approximation.
            pretty_assertions::assert_eq!(parsed.metres().to_bits(), expected_metres.to_bits(), "{input} metres");
            pretty_assertions::assert_eq!(parsed.to_string(), canonical, "{input} round-trip");
        }
    }

    #[test]
    fn a_value_outside_the_rfc_8259_number_grammar_is_a_syntax_error() {
        for input in ["+500", ".5", "5.", "007", "1,000", "", "far"] {
            pretty_assertions::assert_eq!(
                PositiveDistance::from_str(input).expect_err(input),
                PositiveDistanceParseError::InvalidSyntax(input.to_string()),
                "{input} must be a syntax error"
            );
        }
    }

    #[test]
    fn a_non_finite_spelling_is_rejected() {
        // These are not RFC 8259 numbers, so they fail syntactically rather than
        // reaching the finiteness check; the point is they never become a value.
        for input in ["NaN", "inf", "infinity", "-inf"] {
            assert!(PositiveDistance::from_str(input).is_err(), "{input} must be rejected");
        }
    }

    #[test]
    fn a_non_positive_value_is_rejected() {
        for input in ["0", "0.0", "-5"] {
            pretty_assertions::assert_eq!(
                PositiveDistance::from_str(input).expect_err(input),
                PositiveDistanceParseError::NonPositive(input.to_string()),
                "{input} must be non-positive"
            );
        }
    }
}
