//! A `DateTime` member a client wrote, kept in the spelling the client wrote it.
//!
//! Clause 4.6.4 makes the broker "preserve the representation of the content of
//! the values" it is given, so the stored document keeps an `observedAt`, an
//! `expiresAt` (clause 4.22) or a `deletedAt` in the spelling of the request:
//! `.50Z` stays `.50Z`. The spelling is read by [`NgsiLdDateTime`]'s rules
//! first, so what is kept is always a clause 4.6.3 `DateTime`, with only the
//! compatibility comma written back as the decimal point. The text is what is
//! written back; the instant is what expiry and recency comparisons read.

use crate::time::{
    date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
    lexical::with_decimal_point,
    rendering::instant_text,
    resolution::to_microsecond,
};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use derive_more::Display;
use std::str::FromStr;

/// The spelling of a `DateTime` member and the instant it names.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("{text}")]
pub struct DateTimeText {
    /// The spelling as written, with a comma separator written as the point.
    text: CompactString,
    /// The UTC instant the spelling names.
    instant: DateTime<Utc>,
}

impl DateTimeText {
    /// A `DateTime` the broker writes on the client's behalf from an instant it
    /// holds (the Table 6.5.3.4-1 `observedAt` request parameter, a deletion
    /// instant), spelled as every instant the broker emits is spelled.
    #[must_use]
    pub fn from_instant(instant: DateTime<Utc>) -> DateTimeText {
        let instant = to_microsecond(instant);
        DateTimeText {
            text: CompactString::from(instant_text(instant)),
            instant,
        }
    }

    /// The spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The instant the spelling names.
    #[must_use]
    pub const fn instant(&self) -> DateTime<Utc> {
        self.instant
    }
}

impl FromStr for DateTimeText {
    type Err = NgsiLdDateTimeError;

    fn from_str(text: &str) -> Result<DateTimeText, NgsiLdDateTimeError> {
        let instant = text.parse::<NgsiLdDateTime>()?;
        Ok(DateTimeText {
            text: CompactString::from(with_decimal_point(text)),
            instant: instant.get(),
        })
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter serves callers that already own the text.
impl TryFrom<String> for DateTimeText {
    type Error = NgsiLdDateTimeError;

    fn try_from(text: String) -> Result<DateTimeText, NgsiLdDateTimeError> {
        text.parse()
    }
}

#[cfg(test)]
mod tests {
    use crate::time::{date_time::NgsiLdDateTimeError, lexical::LexicalError, written_date_time::DateTimeText};
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};

    fn at(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).single().expect("a valid instant")
    }

    /// Clause 4.6.4: the representation the client wrote is preserved.
    #[test]
    fn the_written_spelling_is_kept_beside_the_instant() {
        let text: DateTimeText = "2026-03-01T09:00:00.50Z".parse().expect("a DateTime");
        pretty_assertions::assert_eq!(text.as_str(), "2026-03-01T09:00:00.50Z");
        pretty_assertions::assert_eq!(text.instant(), at(2026, 3, 1, 9) + TimeDelta::milliseconds(500));
    }

    /// Clause 4.6.3: a comma separator is accepted in requests and kept as the
    /// decimal point.
    #[test]
    fn a_comma_fraction_is_kept_with_a_decimal_point() {
        let text: DateTimeText = "2024-01-01T00:00:00,25Z".parse().expect("a comma fraction");
        pretty_assertions::assert_eq!(text.as_str(), "2024-01-01T00:00:00.25Z");
    }

    /// Clause 4.6.3 holds for a kept spelling exactly as for any other.
    #[test]
    fn a_spelling_clause_4_6_3_refuses_is_not_kept() {
        for (text, refusal) in [
            ("2026-03-01T10:00:00+01:00", NgsiLdDateTimeError::Lexical(LexicalError::ZoneDesignator)),
            ("2026-03-01T09:00:00+00:00", NgsiLdDateTimeError::Lexical(LexicalError::ZoneDesignator)),
            (
                "2026-03-04T05:06:07.1234567Z",
                NgsiLdDateTimeError::Lexical(LexicalError::TooManyFractionalDigits),
            ),
            ("2026-03-04T05:06Z", NgsiLdDateTimeError::Lexical(LexicalError::Malformed)),
            ("yesterday", NgsiLdDateTimeError::Lexical(LexicalError::Malformed)),
        ] {
            pretty_assertions::assert_eq!(text.parse::<DateTimeText>(), Err(refusal), "{text}");
            pretty_assertions::assert_eq!(DateTimeText::try_from(text.to_owned()), Err(refusal), "{text}");
        }
    }

    #[test]
    fn six_fractional_digits_are_kept_as_written() {
        let text: DateTimeText = "2026-03-04T05:06:07.123456Z".parse().expect("six fractional digits");
        pretty_assertions::assert_eq!(text.as_str(), "2026-03-04T05:06:07.123456Z");
    }

    #[test]
    fn a_broker_instant_is_spelled_at_its_microsecond() {
        let text = DateTimeText::from_instant(at(2026, 3, 4, 5) + TimeDelta::nanoseconds(250_000_999));
        pretty_assertions::assert_eq!(text.as_str(), "2026-03-04T05:00:00.250Z");
        pretty_assertions::assert_eq!(text.instant(), at(2026, 3, 4, 5) + TimeDelta::milliseconds(250));
    }
}
