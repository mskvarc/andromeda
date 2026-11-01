//! A number a literal declares, finite or one of the three values beyond the
//! decimals.
//!
//! An `xsd:float` or `xsd:double` literal (XML Schema Definition Language 1.1
//! Part 2, sections 3.3.4 and 3.3.5) may name positive infinity, negative
//! infinity or not-a-number, none of which a JSON number carries. A declared
//! number holds them beside the finite decimals, so the one value kind keeps
//! one representation.

use crate::{JsonNumber, JsonNumberError};
use derive_more::{Display, From};

/// A value of the `xsd:float` and `xsd:double` value spaces that is not a
/// decimal, displayed in its XSD lexical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum NonFinite {
    /// Positive infinity.
    #[display("INF")]
    PositiveInfinity,
    /// Negative infinity.
    #[display("-INF")]
    NegativeInfinity,
    /// Not-a-number.
    #[display("NaN")]
    NotANumber,
}

impl From<NonFinite> for f64 {
    fn from(value: NonFinite) -> f64 {
        match value {
            NonFinite::PositiveInfinity => f64::INFINITY,
            NonFinite::NegativeInfinity => f64::NEG_INFINITY,
            NonFinite::NotANumber => f64::NAN,
        }
    }
}

/// A declared number: a finite decimal, or one of the values [`NonFinite`]
/// names.
///
/// A finite value displays as the broker spells a number, a non-finite one in
/// its XSD lexical form (`INF`, `-INF`, `NaN`).
#[derive(Debug, Clone, PartialEq, Eq, Display, From)]
pub enum DeclaredNumber {
    /// A finite decimal.
    Finite(JsonNumber),
    /// A value beyond the decimals.
    NonFinite(NonFinite),
}

impl TryFrom<DeclaredNumber> for JsonNumber {
    type Error = JsonNumberError;

    /// The JSON number a finite declared number is.
    ///
    /// # Errors
    ///
    /// Returns [`JsonNumberError::NotFinite`] for a value beyond the decimals,
    /// which no JSON number carries.
    fn try_from(number: DeclaredNumber) -> Result<JsonNumber, JsonNumberError> {
        match number {
            DeclaredNumber::Finite(number) => Ok(number),
            DeclaredNumber::NonFinite(value) => Err(JsonNumberError::NotFinite { value: f64::from(value) }),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        JsonNumber,
        JsonNumberError,
        declared_number::{DeclaredNumber, NonFinite},
    };
    use std::str::FromStr;

    fn finite(text: &str) -> DeclaredNumber {
        DeclaredNumber::Finite(JsonNumber::from_str(text).expect("a JSON number"))
    }

    #[test]
    fn a_finite_declared_number_is_spelled_as_the_broker_spells_a_number() {
        for text in ["0", "-0", "1", "-1", "0.1", "1.5", "1E-7", "1E300", "-2.5e-400"] {
            let number = JsonNumber::from_str(text).expect("a JSON number");
            pretty_assertions::assert_eq!(finite(text).to_string(), number.to_string(), "{text}");
        }
        pretty_assertions::assert_eq!(finite("1E-7").to_string(), "1E-7");
        pretty_assertions::assert_eq!(finite("-0").to_string(), "0");
    }

    #[test]
    fn a_non_finite_declared_number_is_spelled_in_its_xsd_lexical_form() {
        pretty_assertions::assert_eq!(DeclaredNumber::from(NonFinite::PositiveInfinity).to_string(), "INF");
        pretty_assertions::assert_eq!(DeclaredNumber::from(NonFinite::NegativeInfinity).to_string(), "-INF");
        pretty_assertions::assert_eq!(DeclaredNumber::from(NonFinite::NotANumber).to_string(), "NaN");
    }

    #[test]
    fn only_a_finite_declared_number_is_a_json_number() {
        let number = JsonNumber::from_str("1.5").expect("a JSON number");
        pretty_assertions::assert_eq!(JsonNumber::try_from(DeclaredNumber::from(number.clone())).expect("finite"), number);
        for value in [NonFinite::PositiveInfinity, NonFinite::NegativeInfinity, NonFinite::NotANumber] {
            assert!(
                matches!(JsonNumber::try_from(DeclaredNumber::from(value)), Err(JsonNumberError::NotFinite { .. })),
                "{value}"
            );
        }
    }

    #[test]
    fn a_non_finite_value_is_the_double_of_the_same_name() {
        let positive = f64::from(NonFinite::PositiveInfinity);
        let negative = f64::from(NonFinite::NegativeInfinity);
        assert!(positive.is_infinite() && positive.is_sign_positive(), "{positive}");
        assert!(negative.is_infinite() && negative.is_sign_negative(), "{negative}");
        assert!(f64::from(NonFinite::NotANumber).is_nan());
    }
}
