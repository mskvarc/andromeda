//! The numeric member of the NGSI-LD value family (ETSI GS CIM 009 clause 4.6.3).
//!
//! NGSI-LD inherits its numeric domain from JSON, which places no bound on the
//! magnitude or the number of significant digits a number may carry. Andromeda
//! parses request bodies with `serde_json`'s default numeric model, so a parsed
//! number is exactly one of a signed 64-bit integer, an unsigned 64-bit integer,
//! or an IEEE 754 double. [`JsonNumber`] names that trichotomy so the shape a
//! value arrived in survives into the persistence layer instead of being flattened
//! to a double at the first `as_f64` call.
//!
//! Keeping the shape is what makes lossy conversions refusable. A caller that
//! needs a double asks for one through [`JsonNumber::to_f64_exact`] and gets a
//! typed error when the value has no exact double, rather than a silently rounded
//! result or a substituted zero.
//!
//! The reasoning behind this representation, and the survey of the arbitrary
//! precision crates considered for the `NUMERIC` storage columns the temporal
//! rebuild introduced, are in `docs/exact-numeric-representation.md`.

use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Number;
use thiserror::Error;

/// Errors raised when a JSON number cannot be carried or converted without loss.
#[derive(Debug, Error)]
pub enum JsonNumberError {
    /// The `serde_json` number was none of the three shapes the default numeric
    /// model produces, which is only reachable when the number retains an
    /// arbitrary-precision lexical form.
    #[error("JSON number {value} is neither a 64-bit signed integer, a 64-bit unsigned integer, nor an IEEE 754 double")]
    Unrepresentable {
        /// The number as it was rendered by `serde_json`.
        value: String,
    },
    /// A double was offered that JSON has no literal for.
    #[error("a JSON number cannot carry the non-finite double {value}")]
    NotFinite {
        /// The offending double, rendered for the message.
        value: f64,
    },
    /// The value is exact as it stands but has no exact IEEE 754 double, so
    /// converting it for a double-typed consumer would change it.
    #[error("JSON number {value} has no exact IEEE 754 double representation and would be rounded by the conversion")]
    InexactAsDouble {
        /// The value that resists exact conversion.
        value: JsonNumber,
    },
}

/// A JSON number held in the shape it was parsed in.
///
/// [`JsonNumber::Double`] holds a finite double: JSON has no literal for an
/// infinity or a NaN, and every constructor in this module refuses one.
///
/// The serialised form is a bare JSON number rather than a tagged variant, and
/// the variant order below is what recovers the shape on the way back: a
/// literal an integer accessor answers for is an integer, and only the
/// remainder is a double. A durable temporal intent is carried in this form, so
/// an integer beyond 2^53 survives the queue as exactly itself.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Display, Serialize)]
#[serde(untagged)]
pub enum JsonNumber {
    /// An integer that fits a signed 64-bit integer.
    #[display("{_0}")]
    Signed(i64),
    /// An integer too large for a signed 64-bit integer but within an unsigned one.
    #[display("{_0}")]
    Unsigned(u64),
    /// A number carrying a fraction or an exponent, held as a finite double.
    #[display("{_0}")]
    Double(f64),
}

impl JsonNumber {
    /// The value as an IEEE 754 double, when the conversion changes nothing.
    ///
    /// Integers above 2^53 in magnitude generally have no exact double, and this
    /// is where that is caught. Storage no longer needs it — `value_number` is
    /// `NUMERIC` in both `attribute_history` and `current_attribute_values`, and
    /// takes the decimal literal — so this is for the readers and the numeric
    /// aggregates that genuinely work in doubles: they get an exact one or a
    /// typed refusal, never a silently rounded value.
    ///
    /// # Errors
    ///
    /// Returns [`JsonNumberError::InexactAsDouble`] when the value does not
    /// survive the conversion unchanged.
    pub fn to_f64_exact(self) -> Result<f64, JsonNumberError> {
        match self {
            JsonNumber::Signed(value) => exact_double_of(value as f64, i128::from(value)).ok_or(JsonNumberError::InexactAsDouble { value: self }),
            JsonNumber::Unsigned(value) => exact_double_of(value as f64, i128::from(value)).ok_or(JsonNumberError::InexactAsDouble { value: self }),
            JsonNumber::Double(value) => Ok(value),
        }
    }
}

// Hand-written rather than derived: `derive_more::From` would generate an
// infallible `From<f64>` alongside these two, and a double reaches this type
// only through the finiteness check in the `TryFrom<f64>` impl below.
impl From<i64> for JsonNumber {
    fn from(value: i64) -> JsonNumber {
        JsonNumber::Signed(value)
    }
}

impl From<u64> for JsonNumber {
    fn from(value: u64) -> JsonNumber {
        JsonNumber::Unsigned(value)
    }
}

impl TryFrom<f64> for JsonNumber {
    type Error = JsonNumberError;

    fn try_from(value: f64) -> Result<JsonNumber, JsonNumberError> {
        if value.is_finite() {
            Ok(JsonNumber::Double(value))
        } else {
            Err(JsonNumberError::NotFinite { value })
        }
    }
}

// The borrowed form is the primary one: every call site holds a `&Number`
// borrowed out of a request body, and the owned adapter below defers to it.
impl TryFrom<&Number> for JsonNumber {
    type Error = JsonNumberError;

    fn try_from(number: &Number) -> Result<JsonNumber, JsonNumberError> {
        // Order matters. `as_i64` answers for every integer a signed 64-bit
        // integer covers, `as_u64` for the integers above that, and `as_f64`
        // only for the remainder -- asking `as_f64` first would round every
        // large integer before the integer accessors ever saw it.
        if let Some(value) = number.as_i64() {
            return Ok(JsonNumber::Signed(value));
        }
        if let Some(value) = number.as_u64() {
            return Ok(JsonNumber::Unsigned(value));
        }
        match number.as_f64() {
            Some(value) => JsonNumber::try_from(value),
            None => Err(JsonNumberError::Unrepresentable { value: number.to_string() }),
        }
    }
}

impl TryFrom<Number> for JsonNumber {
    type Error = JsonNumberError;

    fn try_from(number: Number) -> Result<JsonNumber, JsonNumberError> {
        JsonNumber::try_from(&number)
    }
}

/// The double equal to `exact`, or `None` when the conversion loses information.
///
/// The comparison is widened to `i128` on purpose: a float-to-integer `as` cast
/// saturates, so comparing the round-tripped double against an `i64` would report
/// `i64::MAX` and 2^63 as equal.
fn exact_double_of(candidate: f64, exact: i128) -> Option<f64> {
    (candidate.is_finite() && candidate as i128 == exact).then_some(candidate)
}

#[cfg(test)]
mod tests {
    use crate::attribute::json_number::{JsonNumber, JsonNumberError};
    use serde_json::Number;

    #[test]
    fn a_small_integer_keeps_its_signed_shape_and_converts_exactly() {
        let number = JsonNumber::try_from(&Number::from(42_i64)).expect("a signed integer");
        pretty_assertions::assert_eq!(number, JsonNumber::Signed(42));
        pretty_assertions::assert_eq!(number.to_f64_exact().expect("42 has an exact double"), 42.0);
    }

    #[test]
    fn an_integer_above_the_signed_range_keeps_its_unsigned_shape() {
        let raw = u64::try_from(i64::MAX).expect("the signed maximum is non-negative") + 1;
        let number = JsonNumber::try_from(&Number::from(raw)).expect("an unsigned integer");
        pretty_assertions::assert_eq!(number, JsonNumber::Unsigned(raw));
    }

    #[test]
    fn the_largest_exactly_representable_integer_converts_and_the_next_one_does_not() {
        // 2^53 is the last integer every successor of which the double format
        // can no longer name individually.
        let exact = JsonNumber::Signed(9_007_199_254_740_992);
        pretty_assertions::assert_eq!(exact.to_f64_exact().expect("2^53 has an exact double"), 9_007_199_254_740_992.0);

        let inexact = JsonNumber::Signed(9_007_199_254_740_993);
        assert!(matches!(inexact.to_f64_exact(), Err(JsonNumberError::InexactAsDouble { .. })));
    }

    #[test]
    fn the_signed_boundaries_are_not_confused_by_a_saturating_cast() {
        // `i64::MAX as f64` is 2^63, and casting that back saturates to
        // `i64::MAX`, so a naive round trip would call the pair equal.
        assert!(matches!(
            JsonNumber::Signed(i64::MAX).to_f64_exact(),
            Err(JsonNumberError::InexactAsDouble { .. })
        ));
        // `i64::MIN` is a power of two and does survive the round trip.
        pretty_assertions::assert_eq!(
            JsonNumber::Signed(i64::MIN).to_f64_exact().expect("i64::MIN is a power of two"),
            i64::MIN as f64
        );
    }

    #[test]
    fn the_unsigned_maximum_has_no_exact_double() {
        assert!(matches!(
            JsonNumber::Unsigned(u64::MAX).to_f64_exact(),
            Err(JsonNumberError::InexactAsDouble { .. })
        ));
    }

    #[test]
    fn a_decimal_with_an_exponent_is_carried_as_a_double() {
        let number: Number = serde_json::from_str("1.5e3").expect("an exponent literal is a JSON number");
        pretty_assertions::assert_eq!(JsonNumber::try_from(&number).expect("a double"), JsonNumber::Double(1500.0));
    }

    #[test]
    fn a_negative_exponent_decimal_keeps_the_double_its_parser_produced() {
        let number: Number = serde_json::from_str("-2.5e-4").expect("a negative exponent literal is a JSON number");
        let carried = JsonNumber::try_from(&number).expect("a double");
        pretty_assertions::assert_eq!(carried, JsonNumber::Double(-2.5e-4));
        pretty_assertions::assert_eq!(carried.to_f64_exact().expect("a double is already a double"), -2.5e-4);
    }

    #[test]
    fn a_large_integer_literal_is_refused_rather_than_rounded() {
        let number: Number = serde_json::from_str("9007199254740993").expect("a large integer literal is a JSON number");
        let carried = JsonNumber::try_from(&number).expect("an integer");
        pretty_assertions::assert_eq!(carried, JsonNumber::Signed(9_007_199_254_740_993));
        let error = carried.to_f64_exact().expect_err("the value must not be rounded into a double");
        assert!(error.to_string().contains("9007199254740993"), "got {error}");
    }

    #[test]
    fn a_non_finite_double_is_not_a_json_number() {
        assert!(matches!(JsonNumber::try_from(f64::NAN), Err(JsonNumberError::NotFinite { .. })));
        assert!(matches!(JsonNumber::try_from(f64::INFINITY), Err(JsonNumberError::NotFinite { .. })));
    }

    /// The durable temporal intent carries this type, so the shape has to
    /// survive the queue: a large integer must come back an integer rather than
    /// as the double an eager `as_f64` would have produced.
    #[test]
    fn the_serialised_form_is_a_bare_number_that_recovers_its_shape() {
        let cases = [
            (JsonNumber::Signed(9_007_199_254_740_993), "9007199254740993"),
            (JsonNumber::Unsigned(u64::MAX), "18446744073709551615"),
            (JsonNumber::Double(1.0), "1.0"),
            (JsonNumber::Double(-2.5e-4), "-0.00025"),
        ];
        for (number, encoded) in cases {
            pretty_assertions::assert_eq!(serde_json::to_string(&number).expect("a JSON number serialises"), encoded);
            pretty_assertions::assert_eq!(serde_json::from_str::<JsonNumber>(encoded).expect("a JSON number deserialises"), number);
        }
    }

    #[test]
    fn the_owned_and_borrowed_conversions_agree() {
        let number = Number::from(-7_i64);
        pretty_assertions::assert_eq!(JsonNumber::try_from(&number).expect("borrowed"), JsonNumber::try_from(number).expect("owned"));
    }
}
