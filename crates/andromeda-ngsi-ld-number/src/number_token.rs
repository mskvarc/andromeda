//! The syntactic shape of one JSON number token (RFC 8259 section 6).
//!
//! Every question answered here is answered from the token's characters
//! alone: no decimal is materialized and nothing is allocated, so a document
//! walk can ask them of every number it visits.

/// Whether a token carries a leading minus sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    /// The token starts with `-`.
    Negative,
    /// The token starts with a digit.
    NonNegative,
}

/// The letter introducing a token's exponent part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExponentMarker {
    /// `e`, the spelling `serde_json`'s reader gives every exponent it reads.
    Lower,
    /// `E`, the spelling `BigDecimal` gives a small value's exponent.
    Upper,
}

/// Whether a token's exponent part spells its sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExponentSign {
    /// The exponent digits follow a `+` or `-`.
    Explicit,
    /// The exponent digits follow the marker directly.
    Implicit,
}

/// How a token's exponent part is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Exponent {
    /// The letter introducing the exponent.
    marker: ExponentMarker,
    /// Whether the exponent spells its sign.
    sign: ExponentSign,
}

/// A JSON number token split into the parts RFC 8259 section 6 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NumberToken<'a> {
    /// The whole token.
    text: &'a str,
    /// The optional minus sign.
    sign: Sign,
    /// The integer part: `0` or a digit run without a leading zero.
    integer: &'a str,
    /// The digits after the decimal point, when there is one.
    fraction: Option<&'a str>,
    /// The exponent part's spelling, when there is one.
    exponent: Option<Exponent>,
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
            None
        } else {
            let (marker, exponent) = rest
                .strip_prefix('e')
                .map(|exponent| (ExponentMarker::Lower, exponent))
                .or_else(|| rest.strip_prefix('E').map(|exponent| (ExponentMarker::Upper, exponent)))?;
            let (sign, digits) = exponent
                .strip_prefix(['+', '-'])
                .map_or((ExponentSign::Implicit, exponent), |digits| (ExponentSign::Explicit, digits));
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some(Exponent { marker, sign })
        };
        Some(NumberToken {
            text,
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
    /// configuration renders in plain notation are recognised -- no exponent,
    /// no trailing zero, and at most one zero between the decimal point and
    /// the first significant digit of a value below one -- so a token this
    /// answers `false` for may still be canonical and merely gets re-rendered.
    pub(crate) fn is_canonical(&self) -> bool {
        if self.exponent.is_some() {
            return false;
        }
        match self.fraction {
            None if self.integer == "0" => self.sign == Sign::NonNegative,
            None => !self.integer.ends_with('0'),
            Some(fraction) => !fraction.ends_with('0') && (self.integer != "0" || fraction.bytes().take_while(|digit| *digit == b'0').count() <= 1),
        }
    }

    /// Whether `serde_json`'s reader gives this token back byte for byte when
    /// it reads the token's own serialization.
    ///
    /// The reader spells every exponent `e` followed by an explicit sign, and
    /// reads `-0` as the 64-bit integer zero, which it spells `0`. Every other
    /// token it keeps.
    pub(crate) fn reads_back_unchanged(&self) -> bool {
        self.text != "-0"
            && self.exponent.is_none_or(|exponent| {
                exponent
                    == Exponent {
                        marker: ExponentMarker::Lower,
                        sign: ExponentSign::Explicit,
                    }
            })
    }

    /// Whether `serde_json` hands this token to a visitor as a 64-bit integer.
    ///
    /// Its reader and its value deserializer agree on exactly these tokens: a
    /// non-negative one that parses as `u64` and a negative one that parses as
    /// `i64`. For every other token the two take different visitor paths.
    pub(crate) fn is_64_bit_integer(&self) -> bool {
        self.fraction.is_none()
            && self.exponent.is_none()
            && match self.sign {
                Sign::NonNegative => self.text.parse::<u64>().is_ok(),
                Sign::Negative => self.text.parse::<i64>().is_ok(),
            }
    }
}

/// Split the leading ASCII digit run off `text`.
fn split_digits(text: &str) -> (&str, &str) {
    text.split_at(text.bytes().take_while(u8::is_ascii_digit).count())
}

#[cfg(test)]
mod tests {
    use crate::{JsonNumber, number_token::NumberToken};
    use serde_json::Value;
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

    #[test]
    fn reading_back_unchanged_matches_what_serde_json_reads_from_the_token() {
        for token in VALID_TOKENS {
            let reread = serde_json::from_str::<Value>(token).expect("a valid token").to_string();
            let parsed = NumberToken::parse(token).expect("a valid token");
            pretty_assertions::assert_eq!(parsed.reads_back_unchanged(), reread == token, "{token} reads back as {reread}");
        }
    }

    #[test]
    fn only_integers_inside_the_u64_or_i64_range_are_64_bit_integers() {
        let integers = ["0", "-0", "5", "-5", "18446744073709551615", "9223372036854775807", "-9223372036854775808"];
        let others = [
            "18446744073709551616",
            "-9223372036854775809",
            "0.1",
            "1e2",
            "1.0",
            "340282366920938463463374607431768211456",
        ];
        assert!(
            integers
                .iter()
                .all(|token| NumberToken::parse(token).is_some_and(|parsed| parsed.is_64_bit_integer()))
        );
        assert!(
            others
                .iter()
                .all(|token| NumberToken::parse(token).is_some_and(|parsed| !parsed.is_64_bit_integer()))
        );
    }
}
