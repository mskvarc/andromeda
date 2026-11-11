//! An NGSI-LD `DateTime` held with the number of fractional-second digits it
//! was written with, so it is served as written.
//!
//! Every other instant the broker serves has one spelling ([`instant_text`]).
//! A time a signature covers cannot: ETSI TS 104 179 V1.1.1 clause 5.1.2 has
//! the `created` of a Data Integrity proof be a `DateTime` (clause 4.6.3), and
//! the RDF Dataset Canonicalization a proof is verified by keeps the lexical
//! form of a literal, so `…:24.5Z` and `…:24.500Z` are different signed data.
//! The decimal separator is always written as the point: clause 4.6.3 admits
//! the comma in requests only.
//!
//! [`instant_text`]: crate::time::rendering::instant_text

use crate::time::{
    date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
    lexical::{self, MAX_FRACTIONAL_DIGITS},
    rendering::instant_text_at_width,
};
use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// An NGSI-LD `DateTime` and the number of fractional-second digits it is
/// spelled with.
///
/// [`Display`] writes the instant with exactly that many digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display("{}", instant_text_at_width(instant.get(), *fractional_digits))]
pub struct SpelledDateTime {
    /// The instant.
    instant: NgsiLdDateTime,
    /// How many fractional-second digits the instant is spelled with, zero to
    /// six.
    fractional_digits: u8,
}

/// Why a text or a stored instant is not a [`SpelledDateTime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SpelledDateTimeError {
    /// The text is not an NGSI-LD `DateTime`.
    #[error(transparent)]
    DateTime(#[from] NgsiLdDateTimeError),
    /// More fractional digits than clause 4.6.3 admits.
    #[error("a DateTime is spelled with at most {MAX_FRACTIONAL_DIGITS} fractional-second digits, not {0}")]
    TooManyFractionalDigits(u8),
    /// The instant carries a fraction finer than the digits it is to be spelled
    /// with, so the spelling would name another instant.
    #[error("the instant is finer than its {0} fractional-second digits spell")]
    FinerThanSpelled(u8),
}

impl SpelledDateTime {
    /// The instant spelled with `fractional_digits` digits.
    ///
    /// # Errors
    ///
    /// Returns [`SpelledDateTimeError::TooManyFractionalDigits`] beyond six
    /// digits, and [`SpelledDateTimeError::FinerThanSpelled`] when the instant
    /// carries a fraction those digits cannot spell.
    pub fn new(instant: NgsiLdDateTime, fractional_digits: u8) -> Result<SpelledDateTime, SpelledDateTimeError> {
        let width = usize::from(fractional_digits);
        if width > MAX_FRACTIONAL_DIGITS {
            return Err(SpelledDateTimeError::TooManyFractionalDigits(fractional_digits));
        }
        let unspelled = (width..MAX_FRACTIONAL_DIGITS).fold(1_u32, |scale, _| scale * 10);
        if !instant.get().timestamp_subsec_micros().is_multiple_of(unspelled) {
            return Err(SpelledDateTimeError::FinerThanSpelled(fractional_digits));
        }
        Ok(SpelledDateTime { instant, fractional_digits })
    }

    /// The instant.
    #[must_use]
    pub const fn instant(&self) -> NgsiLdDateTime {
        self.instant
    }

    /// How many fractional-second digits the instant is spelled with.
    #[must_use]
    pub const fn fractional_digits(&self) -> u8 {
        self.fractional_digits
    }
}

// Validating parser: the clause 4.6.3 reader hands on the written fraction
// width beside the instant, which no derive reaches.
impl FromStr for SpelledDateTime {
    type Err = SpelledDateTimeError;

    fn from_str(text: &str) -> Result<SpelledDateTime, SpelledDateTimeError> {
        let (date, time) = lexical::date_time(text).map_err(NgsiLdDateTimeError::from)?;
        let instant = NgsiLdDateTime::from_components(date, time)?;
        SpelledDateTime::new(instant, time.fractional_digits)
    }
}

#[cfg(test)]
mod tests {
    use crate::time::{
        date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
        lexical::LexicalError,
        spelled_date_time::{SpelledDateTime, SpelledDateTimeError},
    };

    fn spelled(text: &str) -> SpelledDateTime {
        text.parse().expect("an NGSI-LD DateTime")
    }

    /// The broker spells a whole millisecond with three digits elsewhere, so
    /// each width below is one the plain rendering would change.
    #[test]
    fn every_width_is_written_back_as_written() {
        for text in [
            "2025-01-27T21:02:24Z",
            "2025-01-27T21:02:24.5Z",
            "2025-01-27T21:02:24.50Z",
            "2025-01-27T21:02:24.500Z",
            "2025-01-27T21:02:24.0500Z",
            "2025-01-27T21:02:24.00000Z",
            "2025-01-27T21:02:24.123456Z",
            "0099-01-01T00:00:00.1Z",
        ] {
            pretty_assertions::assert_eq!(spelled(text).to_string(), text);
        }
    }

    /// Clause 4.6.3 admits the comma in requests only, so it is served as the
    /// decimal point.
    #[test]
    fn a_comma_is_written_back_as_the_decimal_point() {
        pretty_assertions::assert_eq!(spelled("2025-01-27T21:02:24,50Z").to_string(), "2025-01-27T21:02:24.50Z");
    }

    #[test]
    fn the_instant_and_width_are_read_from_the_text() {
        let value = spelled("2025-01-27T21:02:24.50Z");
        pretty_assertions::assert_eq!(value.instant(), "2025-01-27T21:02:24.5Z".parse::<NgsiLdDateTime>().expect("a DateTime"));
        pretty_assertions::assert_eq!(value.fractional_digits(), 2);
    }

    /// TS 104 179 clause 5.1.2 has `created` be a clause 4.6.3 `DateTime`, so
    /// an offset is refused even where XSD would admit it.
    #[test]
    fn an_offset_is_refused() {
        pretty_assertions::assert_eq!(
            "2025-01-27T23:02:24+02:00".parse::<SpelledDateTime>(),
            Err(SpelledDateTimeError::DateTime(NgsiLdDateTimeError::Lexical(LexicalError::ZoneDesignator)))
        );
    }

    #[test]
    fn a_stored_instant_and_width_make_the_value_back() {
        let value = spelled("2025-01-27T21:02:24.250Z");
        pretty_assertions::assert_eq!(SpelledDateTime::new(value.instant(), value.fractional_digits()), Ok(value));
    }

    #[test]
    fn a_width_beyond_six_is_refused() {
        let instant = spelled("2025-01-27T21:02:24Z").instant();
        pretty_assertions::assert_eq!(SpelledDateTime::new(instant, 7), Err(SpelledDateTimeError::TooManyFractionalDigits(7)));
    }

    #[test]
    fn a_width_too_narrow_for_the_instant_is_refused() {
        let instant = spelled("2025-01-27T21:02:24.25Z").instant();
        pretty_assertions::assert_eq!(SpelledDateTime::new(instant, 1), Err(SpelledDateTimeError::FinerThanSpelled(1)));
    }
}
