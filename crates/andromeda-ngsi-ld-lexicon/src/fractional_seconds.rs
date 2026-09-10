//! The clause 4.6.3 fractional-seconds rule, shared by the `DateTime` and `Time` lexical forms.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 states the same sentence twice, once for
//! `DateTime` and once for `Time`: "The Seconds component may optionally contain
//! a decimal fraction. In this case the string shall contain two integer digits,
//! followed by a decimal point and then one or more fractional digits, up to a
//! maximum of six. [...] In requests, also a comma instead of a decimal point may
//! be used as separator for compatibility reasons."
//!
//! Both lexical forms therefore need the same two steps before handing text to a
//! date-time parser: rewrite a compatibility comma to the dot the parser expects,
//! and refuse a fraction longer than the clause admits. They live here rather
//! than in either lexical form so the two cannot drift apart.

use std::borrow::Cow;
use thiserror::Error;

/// The maximum number of fractional-second digits clause 4.6.3 admits.
pub const MAX_FRACTIONAL_DIGITS: usize = 6;

/// Ways the fractional-seconds part of a clause 4.6.3 value can be unusable.
///
/// The variants carry no payload -- the rejected text belongs to the lexical
/// form that was being parsed, which reports it in its own vocabulary -- so the
/// type is a plain tag and copies rather than moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FractionalSecondsError {
    /// More than one comma, which no reading of the compatibility note admits.
    #[error("value carries more than one comma decimal separator")]
    AmbiguousSeparator,
    /// A fraction longer than the clause 4.6.3 maximum.
    #[error("value carries more than {MAX_FRACTIONAL_DIGITS} fractional-second digits")]
    TooManyDigits,
}

/// Rewrite a client-supplied comma decimal separator to the canonical dot.
///
/// Exactly one comma is the form the clause 4.6.3 compatibility note describes;
/// text with more than one comma can never be a valid time value either way, so
/// it is reported rather than silently collapsed.
///
/// # Errors
///
/// Returns [`FractionalSecondsError::AmbiguousSeparator`] when the text carries
/// more than one comma.
pub fn accept_comma_decimal_separator(value: &str) -> Result<Cow<'_, str>, FractionalSecondsError> {
    match value.matches(',').count() {
        0 => Ok(Cow::Borrowed(value)),
        1 => Ok(Cow::Owned(value.replacen(',', ".", 1))),
        _ => Err(FractionalSecondsError::AmbiguousSeparator),
    }
}

/// Refuse a dot-normalised candidate whose fraction exceeds the clause 4.6.3 maximum.
///
/// # Errors
///
/// Returns [`FractionalSecondsError::TooManyDigits`] when the candidate declares
/// more than [`MAX_FRACTIONAL_DIGITS`] fractional-second digits.
pub fn reject_excess_fractional_digits(candidate: &str) -> Result<(), FractionalSecondsError> {
    match fractional_digit_count(candidate) {
        Some(count) if count > MAX_FRACTIONAL_DIGITS => Err(FractionalSecondsError::TooManyDigits),
        Some(_) | None => Ok(()),
    }
}

/// Count the fractional-second digits in a dot-normalised candidate, if it declares any.
fn fractional_digit_count(candidate: &str) -> Option<usize> {
    let (_, after_dot) = candidate.split_once('.')?;
    Some(after_dot.chars().take_while(char::is_ascii_digit).count())
}

#[cfg(test)]
mod tests {
    use crate::fractional_seconds::{FractionalSecondsError, accept_comma_decimal_separator, reject_excess_fractional_digits};

    #[test]
    fn a_single_comma_is_rewritten_to_the_canonical_decimal_point() {
        let rewritten = accept_comma_decimal_separator("12:00:00,5Z").expect("one comma is the compatibility form");
        pretty_assertions::assert_eq!(rewritten.as_ref(), "12:00:00.5Z");
    }

    #[test]
    fn text_without_a_comma_is_borrowed_unchanged() {
        let borrowed = accept_comma_decimal_separator("12:00:00.5Z").expect("no comma needs no rewrite");
        pretty_assertions::assert_eq!(borrowed.as_ref(), "12:00:00.5Z");
    }

    #[test]
    fn more_than_one_comma_is_refused_rather_than_collapsed() {
        let error = accept_comma_decimal_separator("12,00,00Z").expect_err("two commas name no time value");
        assert!(matches!(error, FractionalSecondsError::AmbiguousSeparator));
    }

    #[test]
    fn a_fraction_within_the_clause_maximum_is_accepted() {
        reject_excess_fractional_digits("12:00:00.123456Z").expect("six digits is the clause maximum");
        reject_excess_fractional_digits("12:00:00.5Z").expect("one digit is the clause minimum");
        reject_excess_fractional_digits("12:00:00Z").expect("no fraction at all is admitted");
    }

    #[test]
    fn a_fraction_beyond_the_clause_maximum_is_refused() {
        let error = reject_excess_fractional_digits("12:00:00.1234567Z").expect_err("seven digits exceeds the clause maximum");
        assert!(matches!(error, FractionalSecondsError::TooManyDigits));
    }
}
