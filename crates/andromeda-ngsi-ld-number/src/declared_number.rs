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
use std::str::FromStr;

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

impl DeclaredNumber {
    /// The number an XSD 1.1 lexical form of `xsd:decimal`, `xsd:float`,
    /// `xsd:double` or a datatype derived from them names (XML Schema
    /// Definition Language 1.1 Part 2, sections 3.3.3 to 3.3.5).
    ///
    /// The lexical forms are wider than JSON numbers: a leading `+`, leading
    /// zeros, a fraction with no integer digits (`.5`) or no fractional digits
    /// (`5.`), and the three spellings `INF` (also `+INF`), `-INF` and `NaN`.
    /// The text is brought to JSON number syntax and read exactly, so
    /// `"+1.50"` is `1.5` and `"1.0E2"` is `100`.
    ///
    /// # Errors
    ///
    /// Returns [`JsonNumberError::NotXsdLexical`] for a mantissa that is no
    /// such lexical form, [`JsonNumberError::InvalidSyntax`] for a malformed
    /// exponent, and the range refusals of [`JsonNumber`] for a number the
    /// store cannot hold.
    pub fn from_xsd_lexical(text: &str) -> Result<DeclaredNumber, JsonNumberError> {
        match text {
            "INF" | "+INF" => return Ok(DeclaredNumber::NonFinite(NonFinite::PositiveInfinity)),
            "-INF" => return Ok(DeclaredNumber::NonFinite(NonFinite::NegativeInfinity)),
            "NaN" => return Ok(DeclaredNumber::NonFinite(NonFinite::NotANumber)),
            _ => {}
        }
        let (sign, unsigned) = match text.strip_prefix('-') {
            Some(unsigned) => ("-", unsigned),
            None => ("", text.strip_prefix('+').unwrap_or(text)),
        };
        let (mantissa, exponent) = unsigned
            .split_once(['e', 'E'])
            .map_or((unsigned, None), |(mantissa, exponent)| (mantissa, Some(exponent)));
        let (integral, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if (integral.is_empty() && fraction.is_empty()) || !digits(integral) || !digits(fraction) {
            return Err(JsonNumberError::NotXsdLexical);
        }
        let integral = integral.trim_start_matches('0');
        let mut json = String::with_capacity(text.len() + 2);
        json.push_str(sign);
        json.push_str(if integral.is_empty() { "0" } else { integral });
        if !fraction.is_empty() {
            json.push('.');
            json.push_str(fraction);
        }
        if let Some(exponent) = exponent {
            json.push('e');
            json.push_str(exponent);
        }
        JsonNumber::from_str(&json).map(DeclaredNumber::Finite)
    }
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
    fn an_xsd_lexical_form_reads_as_the_number_it_names() {
        for (text, expected) in [
            ("01", "1"),
            ("+1.50", "1.5"),
            ("1.0", "1"),
            ("-0", "0"),
            (".5", "0.5"),
            ("5.", "5"),
            ("-.25", "-0.25"),
            ("1.0E2", "100"),
            ("0.0000001", "1E-7"),
            ("+2e+3", "2E+3"),
            ("000", "0"),
        ] {
            pretty_assertions::assert_eq!(DeclaredNumber::from_xsd_lexical(text).expect("a lexical form"), finite(expected), "{text}");
        }
    }

    #[test]
    fn the_xsd_spellings_beyond_the_decimals_read_as_their_values() {
        for (text, value) in [
            ("INF", NonFinite::PositiveInfinity),
            ("+INF", NonFinite::PositiveInfinity),
            ("-INF", NonFinite::NegativeInfinity),
            ("NaN", NonFinite::NotANumber),
        ] {
            pretty_assertions::assert_eq!(
                DeclaredNumber::from_xsd_lexical(text).expect("a lexical form"),
                DeclaredNumber::NonFinite(value),
                "{text}"
            );
        }
    }

    #[test]
    fn a_text_that_is_no_xsd_number_is_refused() {
        for text in ["", "abc", "1,5", "inf", "nan", "1e", "--1", ".", "+", "1.2.3", "+-1"] {
            assert!(DeclaredNumber::from_xsd_lexical(text).is_err(), "{text}");
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
