//! The NGSI-LD `Time` (ETSI GS CIM 009 v1.9.1 clause 4.6.3): a time of day in UTC at the microsecond.
//!
//! Clause 4.6.3 writes a `Time` as `hh:mm:ssZ`, with the same optional fraction
//! as a `DateTime`, and states that "the string shall not contain expressions
//! of the difference between local time and UTC", so `Z` is the only designator
//! a value may end in. The fraction may be absent or one to six digits, so text
//! order over the served spelling (none, three or six digits) is not
//! chronological: `12:00:00.500Z` sorts before `12:00:00Z` byte-wise and after
//! it in time.
//!
//! [`NgsiLdTime`]'s [`Display`] is therefore the storage encoding of a time of
//! day, `HH:MM:SS.ffffffZ`, which no client reads. Fixed width plus a fixed
//! designator makes byte-wise (`COLLATE "C"`) order over the stored text
//! chronological, which is what lets clause 4.9's `<`, `>`, `<=` and `>=` be
//! answered by a plain string comparison and by the B-tree index over it.

use crate::time::{
    lexical::{self, LexicalError},
    rendering::fixed_width_time_of_day_text,
    resolution::to_microsecond,
};
use chrono::NaiveTime;
use derive_more::{AsRef, Deref, Display};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// Why a text is not an NGSI-LD `Time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NgsiLdTimeError {
    /// The text is not written in the clause 4.6.3 `Time` form.
    #[error("NGSI-LD Time value {0}")]
    Lexical(#[from] LexicalError),
    /// The text is written in the form but names no time of day.
    #[error("NGSI-LD Time value names no time of day")]
    OutOfRange,
}

/// An NGSI-LD `Time` at microsecond resolution.
///
/// [`Display`] renders the fixed-width storage encoding `HH:MM:SS.ffffffZ`. Two
/// values compare through [`Ord`] in chronological order, and their stored
/// spellings compare byte-wise in the same order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Display, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
#[display("{}", fixed_width_time_of_day_text(*_0))]
pub struct NgsiLdTime(NaiveTime);

impl NgsiLdTime {
    /// The time of day, in UTC.
    #[must_use]
    pub const fn get(self) -> NaiveTime {
        self.0
    }

    /// Bring a time of day the broker computed (a mean, say) to microsecond
    /// resolution, truncating any finer remainder.
    #[must_use]
    pub fn normalise(time: NaiveTime) -> NgsiLdTime {
        NgsiLdTime(to_microsecond(time))
    }
}

// Validating parser: the clause 4.6.3 grammar and the clock check are steps no
// derive expresses, and `derive_more::FromStr` would delegate to `NaiveTime`'s
// own reader, which requires no designator at all.
impl FromStr for NgsiLdTime {
    type Err = NgsiLdTimeError;

    fn from_str(text: &str) -> Result<NgsiLdTime, NgsiLdTimeError> {
        let time = lexical::time(text)?;
        NaiveTime::from_hms_micro_opt(time.hour, time.minute, time.second, time.microsecond)
            .map(NgsiLdTime)
            .ok_or(NgsiLdTimeError::OutOfRange)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter is what `#[serde(try_from = "String")]` requires.
impl TryFrom<String> for NgsiLdTime {
    type Error = NgsiLdTimeError;

    fn try_from(text: String) -> Result<NgsiLdTime, NgsiLdTimeError> {
        text.parse()
    }
}

// `derive_more::Into` would yield `Into<NaiveTime>`, the inner type. What
// `#[serde(into = "String")]` needs is the stored spelling, which no derive
// produces from a `chrono` value.
impl From<NgsiLdTime> for String {
    fn from(time: NgsiLdTime) -> String {
        time.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::time::{
        lexical::LexicalError,
        time_of_day::{NgsiLdTime, NgsiLdTimeError},
    };
    use chrono::{NaiveTime, Timelike};

    #[test]
    fn a_whole_second_is_stored_with_six_fractional_digits() {
        let parsed: NgsiLdTime = "12:00:00Z".parse().expect("a Time");
        pretty_assertions::assert_eq!(parsed.to_string(), "12:00:00.000000Z");
    }

    /// Clause 4.6.3, `Time`: "The string shall not contain expressions of the
    /// difference between local time and UTC." `+00:00` is such an expression
    /// even though it names no difference.
    #[test]
    fn a_numeric_offset_or_a_lowercase_designator_is_refused() {
        for text in ["13:00:00+01:00", "23:30:00-05:00", "12:00:00+00:00", "12:00:00z"] {
            pretty_assertions::assert_eq!(
                text.parse::<NgsiLdTime>(),
                Err(NgsiLdTimeError::Lexical(LexicalError::ZoneDesignator)),
                "{text}"
            );
        }
    }

    /// Clause 4.6.3, `Time`: "In requests, also a comma instead of a decimal
    /// point may be used as separator for compatibility reasons."
    #[test]
    fn a_comma_separator_names_the_same_time_of_day() {
        let comma: NgsiLdTime = "12:00:00,5Z".parse().expect("the comma form");
        let point: NgsiLdTime = "12:00:00.5Z".parse().expect("the decimal point form");
        pretty_assertions::assert_eq!(comma, point);
        pretty_assertions::assert_eq!(comma.to_string(), "12:00:00.500000Z");
    }

    #[test]
    fn a_seventh_fractional_digit_is_refused() {
        pretty_assertions::assert_eq!(
            "12:00:00.1234567Z".parse::<NgsiLdTime>(),
            Err(NgsiLdTimeError::Lexical(LexicalError::TooManyFractionalDigits))
        );
    }

    #[test]
    fn a_time_without_its_designator_or_with_missing_components_is_malformed() {
        for text in ["12:00:00", "12:00Z", "noon", "2024-01-01T12:00:00Z"] {
            pretty_assertions::assert_eq!(text.parse::<NgsiLdTime>(), Err(NgsiLdTimeError::Lexical(LexicalError::Malformed)), "{text}");
        }
    }

    #[test]
    fn a_field_outside_the_clock_is_out_of_range() {
        for text in ["25:00:00Z", "12:60:00Z", "12:00:60Z"] {
            pretty_assertions::assert_eq!(text.parse::<NgsiLdTime>(), Err(NgsiLdTimeError::OutOfRange), "{text}");
        }
    }

    #[test]
    fn a_computed_time_of_day_is_truncated_to_the_microsecond() {
        let time = NaiveTime::from_hms_nano_opt(12, 0, 0, 123_456_900).expect("a time of day");
        let normalised = NgsiLdTime::normalise(time);
        pretty_assertions::assert_eq!(normalised.get().nanosecond(), 123_456_000);
        pretty_assertions::assert_eq!(NgsiLdTime::normalise(normalised.get()), normalised);
    }

    /// The property the stored spelling exists for: byte-wise order over it is
    /// chronological order, which is what clause 4.9's ordering operators are
    /// answered by once the value is stored as text.
    #[test]
    fn byte_wise_order_over_the_stored_spelling_is_chronological_order() {
        let ordered = [
            "00:00:00Z",
            "00:00:00.000001Z",
            "01:02:03.5Z",
            "09:00:00Z",
            "10:00:00Z",
            "11:00:00,5Z",
            "12:00:00Z",
            "23:59:59.999999Z",
        ];
        let parsed: Vec<NgsiLdTime> = ordered.iter().map(|value| value.parse().expect("a Time")).collect();
        for window in parsed.windows(2) {
            let (earlier, later) = (window[0], window[1]);
            assert!(earlier < later, "{earlier} must precede {later} chronologically");
            assert!(earlier.to_string() < later.to_string(), "{earlier} must precede {later} byte-wise as well");
        }
    }

    #[test]
    fn an_owned_string_parses_through_the_try_from_adapter() {
        let parsed = NgsiLdTime::try_from("12:00:00Z".to_owned()).expect("an owned Time");
        pretty_assertions::assert_eq!(String::from(parsed), "12:00:00.000000Z");
    }
}
