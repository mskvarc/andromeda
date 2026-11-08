//! The syntactic shape of one JSON number token (RFC 8259 section 6).
//!
//! Every question answered here is answered from the token's characters
//! alone: no decimal is materialized and nothing is allocated, so a document
//! walk can ask them of every number it visits.

use crate::{MAX_FRACTIONAL_DIGITS, MAX_INTEGRAL_DIGITS};

/// Whether a token carries a leading minus sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    /// The token starts with `-`.
    Negative,
    /// The token starts with a digit.
    NonNegative,
}

/// Whether a token carries an exponent part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exponent {
    /// The token ends after its integer or fraction digits.
    Absent,
    /// The token ends with `e` or `E`, an optional sign and digits.
    Present,
}

/// A JSON number token split into the parts RFC 8259 section 6 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NumberToken<'a> {
    /// The optional minus sign.
    sign: Sign,
    /// The integer part: `0` or a digit run without a leading zero.
    integer: &'a str,
    /// The digits after the decimal point, when there is one.
    fraction: Option<&'a str>,
    /// Whether the token has an exponent part.
    exponent: Exponent,
}

impl<'a> NumberToken<'a> {
    /// Split `text` into its parts.
    ///
    /// Returns `None` when `text` is not RFC 8259 number grammar, which a
    /// `serde_json::Number` built with `from_string_unchecked` can carry.
    pub(crate) fn parse(text: &'a str) -> Option<NumberToken<'a>> {
        let (sign, unsigned) = text.strip_prefix('-').map_or((Sign::NonNegative, text), |rest| (Sign::Negative, rest));
        let (integer, rest) = split_digits(unsigned);
        if integer.is_empty() || (integer.len() > 1 && integer.starts_with('0')) {
            return None;
        }
        let (fraction, rest) = match rest.strip_prefix('.') {
            Some(after_point) => {
                let (fraction, rest) = split_digits(after_point);
                if fraction.is_empty() {
                    return None;
                }
                (Some(fraction), rest)
            }
            None => (None, rest),
        };
        let exponent = if rest.is_empty() {
            Exponent::Absent
        } else {
            let exponent = rest.strip_prefix(['e', 'E'])?;
            let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Exponent::Present
        };
        Some(NumberToken {
            sign,
            integer,
            fraction,
            exponent,
        })
    }

    /// Whether [`JsonNumber`](crate::JsonNumber) renders this token's value
    /// with exactly this spelling.
    ///
    /// `BigDecimal` switches to exponent notation past two zero-run
    /// thresholds that are build-time configurable. Only the spellings every
    /// configuration renders in plain notation are recognised: no exponent,
    /// no trailing zero, and at most one zero between the decimal point and
    /// the first significant digit of a value below one. A token this answers
    /// `false` for may still be canonical and merely gets re-rendered.
    pub(crate) fn is_canonical(&self) -> bool {
        if self.exponent == Exponent::Present {
            return false;
        }
        match self.fraction {
            None if self.integer == "0" => self.sign == Sign::NonNegative,
            None => !self.integer.ends_with('0'),
            Some(fraction) => !fraction.ends_with('0') && (self.integer != "0" || fraction.bytes().take_while(|digit| *digit == b'0').count() <= 1),
        }
    }

    /// Whether this token carries no exponent and its value lies within the
    /// PostgreSQL `NUMERIC` range, decided from its digits alone.
    ///
    /// Without an exponent, the integer digit run bounds the integral digits
    /// and the fraction digit run without its trailing zeros is exactly the
    /// fractional digits the range is stated over. A token with an exponent is
    /// left to the exact check.
    pub(crate) fn is_plainly_within_range(&self) -> bool {
        self.exponent == Exponent::Absent
            && i64::try_from(self.integer.len()).is_ok_and(|digits| digits <= MAX_INTEGRAL_DIGITS)
            && i64::try_from(self.fraction.map_or(0, |fraction| fraction.trim_end_matches('0').len())).is_ok_and(|digits| digits <= MAX_FRACTIONAL_DIGITS)
    }
}

/// Split the leading ASCII digit run off `text`.
fn split_digits(text: &str) -> (&str, &str) {
    text.split_at(text.bytes().take_while(u8::is_ascii_digit).count())
}

#[cfg(test)]
mod tests {
    use crate::{JsonNumber, MAX_FRACTIONAL_DIGITS, MAX_INTEGRAL_DIGITS, number_token::NumberToken};
    use std::str::FromStr;

    /// Tokens covering signs, zeros, 64-bit boundaries, fractions and both
    /// exponent spellings.
    const VALID_TOKENS: [&str; 34] = [
        "0",
        "-0",
        "5",
        "-5",
        "10",
        "100",
        "-120",
        "18446744073709551615",
        "18446744073709551616",
        "9223372036854775807",
        "-9223372036854775808",
        "-9223372036854775809",
        "340282366920938463463374607431768211456",
        "0.1",
        "-0.5",
        "0.05",
        "0.005",
        "0.00001",
        "0.0",
        "1.50",
        "21.5",
        "-21.25",
        "0.100000000000000005",
        "1e2",
        "1E2",
        "1e+2",
        "1e-7",
        "1E-7",
        "-2.5E-400",
        "-2.5e-400",
        "0e-999999",
        "-0.0",
        "-0e5",
        "123456789012345678900",
    ];

    #[test]
    fn rfc_8259_number_grammar_is_accepted_and_everything_else_rejected() {
        assert!(VALID_TOKENS.iter().all(|token| NumberToken::parse(token).is_some()));
        for invalid in [
            "", "-", "+1", "01", "-01", "1.", ".5", "1e", "1e+", "1.e5", "1x", "NaN", "inf", "0x10", "1 ", " 1", "1e5.0", "--1",
        ] {
            assert!(NumberToken::parse(invalid).is_none(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn a_token_recognised_as_canonical_is_exactly_how_json_number_renders_its_value() {
        for token in VALID_TOKENS
            .iter()
            .filter(|token| NumberToken::parse(token).is_some_and(|parsed| parsed.is_canonical()))
        {
            let rendered = JsonNumber::from_str(token).expect("a supported number").to_string();
            pretty_assertions::assert_eq!(&rendered, token);
        }
    }

    #[test]
    fn plain_normalized_spellings_are_canonical_and_everything_else_is_re_rendered() {
        let canonical = ["0", "5", "-5", "18446744073709551616", "0.1", "-0.5", "0.05", "21.5", "0.100000000000000005"];
        let re_rendered = [
            "-0",
            "10",
            "100",
            "0.005",
            "0.00001",
            "0.0",
            "1.50",
            "1e2",
            "1E-7",
            "-2.5E-400",
            "-0.0",
            "123456789012345678900",
        ];
        assert!(
            canonical
                .iter()
                .all(|token| NumberToken::parse(token).is_some_and(|parsed| parsed.is_canonical()))
        );
        assert!(
            re_rendered
                .iter()
                .all(|token| NumberToken::parse(token).is_some_and(|parsed| !parsed.is_canonical()))
        );
    }

    /// The plain range check agrees with the exact decimal on every token
    /// without an exponent, and leaves every token with one to it.
    #[test]
    fn a_plain_token_is_in_range_exactly_when_its_decimal_is() {
        let fraction_digits = usize::try_from(MAX_FRACTIONAL_DIGITS).expect("a small constant");
        let at_bound = format!("0.{}1", "0".repeat(fraction_digits - 1));
        let past_bound = format!("0.{}1", "0".repeat(fraction_digits));
        let trailing_zeros = format!("{at_bound}{}", "0".repeat(40));
        let integral = format!("1{}", "0".repeat(usize::try_from(MAX_INTEGRAL_DIGITS).expect("a small constant")));
        for token in VALID_TOKENS
            .iter()
            .copied()
            .chain([at_bound.as_str(), past_bound.as_str(), trailing_zeros.as_str(), integral.as_str(), "13.1000000"])
        {
            let parsed = NumberToken::parse(token).expect("a valid token");
            let exact = JsonNumber::from_str(token).is_ok();
            if parsed.is_plainly_within_range() {
                assert!(exact, "{token}");
            } else {
                assert!(!exact || token.contains(['e', 'E']), "{token}");
            }
        }
    }
}
