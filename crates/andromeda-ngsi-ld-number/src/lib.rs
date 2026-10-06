//! Exact JSON numbers in the NGSI-LD value family (ETSI GS CIM 009 clause 4.6.3).
//!
//! Andromeda preserves the mathematical value of every accepted JSON number.
//! PostgreSQL `NUMERIC` supplies the persistence domain, so accepted non-zero
//! values are limited to 131,072 integral digits and 16,383 fractional digits.

use bigdecimal::{BigDecimal, ParseBigDecimalError, Zero, serde::json_num};
use derive_more::Display;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError, ser::Error as SerializeError};
use serde_json::Number;
use std::{num::ParseIntError, str::FromStr};
use thiserror::Error;

pub mod float_spelling;
pub mod json_value;
mod number_token;

/// Maximum decimal digits PostgreSQL `NUMERIC` supports before the decimal point.
pub const MAX_INTEGRAL_DIGITS: i64 = 131_072;

/// Maximum decimal digits PostgreSQL `NUMERIC` supports after the decimal point.
pub const MAX_FRACTIONAL_DIGITS: i64 = 16_383;

/// Errors raised while constructing an exact JSON number.
#[derive(Debug, Error)]
pub enum JsonNumberError {
    /// The input is not JSON number syntax.
    #[error("invalid JSON number: {source}")]
    InvalidSyntax {
        /// The JSON grammar error.
        source: serde_json::Error,
    },
    /// A syntactically valid JSON number could not be represented as a decimal.
    #[error("invalid decimal value: {source}")]
    InvalidDecimal {
        /// The decimal parsing error.
        source: ParseBigDecimalError,
    },
    /// A non-zero value exceeds PostgreSQL's supported `NUMERIC` range.
    #[error("JSON number exceeds the supported range of {MAX_INTEGRAL_DIGITS} integral digits and {MAX_FRACTIONAL_DIGITS} fractional digits")]
    OutOfSupportedRange,
    /// A floating-point value has no JSON number representation.
    #[error("a JSON number cannot carry the non-finite floating-point value {value}")]
    NotFinite {
        /// The rejected floating-point value.
        value: f64,
    },
}

/// A mathematical decimal value accepted by both JSON and PostgreSQL `NUMERIC`.
///
/// Equality and ordering are value-based: `1`, `1.0`, and `1e0` are the same
/// number. Serialization emits a bare canonical JSON number and does not
/// preserve the spelling used by the client.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Display)]
#[display("{_0}")]
pub struct JsonNumber(BigDecimal);

impl JsonNumber {
    /// Constructs a checked number from an arbitrary-precision decimal.
    ///
    /// # Errors
    ///
    /// Returns [`JsonNumberError::OutOfSupportedRange`] when the normalized
    /// value cannot be stored in PostgreSQL `NUMERIC`.
    pub fn from_decimal(value: &BigDecimal) -> Result<JsonNumber, JsonNumberError> {
        let value = value.normalized();
        validate_decimal_range(&value)?;
        Ok(JsonNumber(value))
    }

    /// Borrows the exact decimal for database bindings and arithmetic.
    #[must_use]
    pub const fn decimal(&self) -> &BigDecimal {
        &self.0
    }

    /// Constructs the native `serde_json` number token for this exact value.
    #[must_use]
    pub fn to_serde_number(&self) -> Number {
        Number::from_string_unchecked(self.to_string())
    }
}

impl Serialize for JsonNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        json_num::serialize(&self.0, serializer).map_err(SerializeError::custom)
    }
}

impl<'de> Deserialize<'de> for JsonNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<JsonNumber, D::Error> {
        let number = Number::deserialize(deserializer)?;
        JsonNumber::try_from(&number).map_err(DeserializeError::custom)
    }
}

impl FromStr for JsonNumber {
    type Err = JsonNumberError;

    fn from_str(value: &str) -> Result<JsonNumber, JsonNumberError> {
        let number = Number::from_str(value).map_err(|source| JsonNumberError::InvalidSyntax { source })?;
        JsonNumber::try_from(&number)
    }
}

impl TryFrom<&Number> for JsonNumber {
    type Error = JsonNumberError;

    fn try_from(number: &Number) -> Result<JsonNumber, JsonNumberError> {
        checked_decimal_from_json(number.as_str())
    }
}

impl TryFrom<Number> for JsonNumber {
    type Error = JsonNumberError;

    fn try_from(number: Number) -> Result<JsonNumber, JsonNumberError> {
        JsonNumber::try_from(&number)
    }
}

impl TryFrom<BigDecimal> for JsonNumber {
    type Error = JsonNumberError;

    fn try_from(value: BigDecimal) -> Result<JsonNumber, JsonNumberError> {
        JsonNumber::from_decimal(&value)
    }
}

impl TryFrom<f64> for JsonNumber {
    type Error = JsonNumberError;

    fn try_from(value: f64) -> Result<JsonNumber, JsonNumberError> {
        let number = Number::from_f64(value).ok_or(JsonNumberError::NotFinite { value })?;
        JsonNumber::try_from(number)
    }
}

macro_rules! impl_integer_conversion {
    ($($integer:ty),+ $(,)?) => {
        $(
            impl From<$integer> for JsonNumber {
                fn from(value: $integer) -> JsonNumber {
                    JsonNumber(BigDecimal::from(value))
                }
            }
        )+
    };
}

// Every primitive integer is many orders of magnitude inside PostgreSQL's
// range, so these conversions are infallible without bypassing validation.
impl_integer_conversion!(i8, i16, i32, i64, i128, u8, u16, u32, u64, u128);

/// Parse a grammar-checked JSON number after validating its range from the
/// compact spelling. A zero bypasses exponent parsing because all spellings of
/// zero normalize to the same supported value, including extreme exponents.
fn checked_decimal_from_json(value: &str) -> Result<JsonNumber, JsonNumberError> {
    let shape = DecimalShape::parse(value)?;
    if shape.is_zero {
        return Ok(JsonNumber(BigDecimal::zero()));
    }
    if shape.integral_digits > MAX_INTEGRAL_DIGITS || shape.fractional_digits > MAX_FRACTIONAL_DIGITS {
        return Err(JsonNumberError::OutOfSupportedRange);
    }
    let decimal = BigDecimal::from_str(value).map_err(|source| JsonNumberError::InvalidDecimal { source })?;
    Ok(JsonNumber(decimal.normalized()))
}

/// Validate a decimal supplied by a non-JSON source without rendering an
/// expanded plain-decimal representation.
fn validate_decimal_range(value: &BigDecimal) -> Result<(), JsonNumberError> {
    if value.is_zero() {
        return Ok(());
    }
    let digits = i64::try_from(value.digits()).map_err(|_| JsonNumberError::OutOfSupportedRange)?;
    let scale = value.fractional_digit_count();
    let integral_digits = digits.checked_sub(scale).unwrap_or(i64::MAX).max(0);
    let fractional_digits = scale.max(0);
    if integral_digits > MAX_INTEGRAL_DIGITS || fractional_digits > MAX_FRACTIONAL_DIGITS {
        Err(JsonNumberError::OutOfSupportedRange)
    } else {
        Ok(())
    }
}

/// Significant digit extents after applying a compact JSON exponent.
struct DecimalShape {
    is_zero: bool,
    integral_digits: i64,
    fractional_digits: i64,
}

impl DecimalShape {
    /// Inspect syntax already accepted by `serde_json::Number` without
    /// materializing the exponent as zero padding.
    fn parse(value: &str) -> Result<DecimalShape, JsonNumberError> {
        let unsigned = value.strip_prefix('-').unwrap_or(value);
        let (mantissa, exponent_text) = unsigned
            .split_once(['e', 'E'])
            .map_or((unsigned, None), |(mantissa, exponent)| (mantissa, Some(exponent)));
        let integral_width = mantissa.find('.').unwrap_or(mantissa.len());
        let digits = mantissa.bytes().filter(u8::is_ascii_digit).collect::<Vec<_>>();
        let Some(first_nonzero) = digits.iter().position(|digit| *digit != b'0') else {
            return Ok(DecimalShape {
                is_zero: true,
                integral_digits: 0,
                fractional_digits: 0,
            });
        };
        let exponent = exponent_text
            .map(parse_exponent)
            .transpose()
            .map_err(|_source| JsonNumberError::OutOfSupportedRange)?
            .unwrap_or(0);
        let last_nonzero = digits.iter().rposition(|digit| *digit != b'0').unwrap_or(first_nonzero);
        let point = i64::try_from(integral_width)
            .ok()
            .and_then(|width| width.checked_add(exponent))
            .ok_or(JsonNumberError::OutOfSupportedRange)?;
        let first_nonzero = i64::try_from(first_nonzero).map_err(|_| JsonNumberError::OutOfSupportedRange)?;
        let after_last_nonzero = i64::try_from(last_nonzero)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or(JsonNumberError::OutOfSupportedRange)?;
        Ok(DecimalShape {
            is_zero: false,
            integral_digits: point.checked_sub(first_nonzero).unwrap_or(i64::MAX).max(0),
            fractional_digits: after_last_nonzero.checked_sub(point).unwrap_or(i64::MAX).max(0),
        })
    }
}

/// Parse a JSON exponent after stripping its optional sign.
fn parse_exponent(value: &str) -> Result<i64, ParseIntError> {
    value.parse::<i64>()
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();

#[cfg(test)]
mod tests {
    use crate::{JsonNumber, JsonNumberError, MAX_FRACTIONAL_DIGITS, MAX_INTEGRAL_DIGITS};
    use std::str::FromStr;

    #[test]
    fn raw_numbers_preserve_their_exact_mathematical_values() {
        let cases = [
            ("0.100000000000000005", "0.100000000000000005"),
            ("18446744073709551616", "18446744073709551616"),
            ("-2.5e-400", "-2.5E-400"),
            ("1.234567890123456789e20", "123456789012345678900"),
        ];
        for (input, expected) in cases {
            let number = JsonNumber::from_str(input).expect("a supported exact JSON number");
            pretty_assertions::assert_eq!(number.decimal().to_string(), expected);
            let encoded = serde_json::to_string(&number).expect("a bare JSON number");
            pretty_assertions::assert_eq!(JsonNumber::from_str(&encoded).expect("the serialized number"), number);
        }
    }

    #[test]
    fn equivalent_spellings_have_value_based_equality_and_canonical_output() {
        let numbers = ["1", "1.0", "1e0"].map(|value| JsonNumber::from_str(value).expect("an equivalent one"));
        assert!(numbers.windows(2).all(|pair| pair[0] == pair[1]));
        pretty_assertions::assert_eq!(serde_json::to_string(&numbers[0]).expect("one"), "1");

        let zeroes = ["-0", "0.000"].map(|value| JsonNumber::from_str(value).expect("an equivalent zero"));
        pretty_assertions::assert_eq!(zeroes[0], zeroes[1]);
        pretty_assertions::assert_eq!(serde_json::to_string(&zeroes[0]).expect("zero"), "0");
    }

    #[test]
    fn postgresql_numeric_boundaries_are_checked_without_expanding_exponents() {
        let largest_integral = "9".repeat(usize::try_from(MAX_INTEGRAL_DIGITS).expect("positive limit"));
        assert!(JsonNumber::from_str(&largest_integral).is_ok());
        let oversized_integral = format!("{largest_integral}9");
        assert!(matches!(JsonNumber::from_str(&oversized_integral), Err(JsonNumberError::OutOfSupportedRange)));
        assert!(JsonNumber::from_str("1e-16383").is_ok());
        assert!(matches!(JsonNumber::from_str("1e-16384"), Err(JsonNumberError::OutOfSupportedRange)));
        assert!(JsonNumber::from_str("0e-999999999999999999999999999999999999").is_ok());
        assert!(matches!(
            JsonNumber::from_str("1e999999999999999999999999999999999999"),
            Err(JsonNumberError::OutOfSupportedRange)
        ));
        pretty_assertions::assert_eq!(MAX_FRACTIONAL_DIGITS, 16_383);
    }

    #[test]
    fn invalid_json_number_syntax_and_non_finite_floats_are_typed_errors() {
        for invalid in ["01", "1e", "NaN", "inf", "+1"] {
            assert!(
                matches!(JsonNumber::from_str(invalid), Err(JsonNumberError::InvalidSyntax { .. })),
                "accepted {invalid}"
            );
        }
        assert!(matches!(JsonNumber::try_from(f64::NAN), Err(JsonNumberError::NotFinite { .. })));
        assert!(matches!(JsonNumber::try_from(f64::INFINITY), Err(JsonNumberError::NotFinite { .. })));
    }

    #[test]
    fn serde_uses_bare_arbitrary_precision_numbers() {
        let input = "0.100000000000000005";
        let number = serde_json::from_str::<JsonNumber>(input).expect("an exact number");
        pretty_assertions::assert_eq!(serde_json::to_string(&number).expect("a JSON number"), input);
    }
}
