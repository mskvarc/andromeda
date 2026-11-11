//! The NGSI-LD `DateTime` (ETSI GS CIM 009 v1.9.1 clause 4.6.3): an instant in UTC at the microsecond.
//!
//! Every text that names an instant reaches the broker through [`NgsiLdDateTime`]'s
//! [`FromStr`]: a `DateTime` Property value, `observedAt`, `expiresAt`, the
//! `timeAt` and `endTimeAt` of a temporal query or deletion, the instants of a
//! Subscription, a Context Source Registration, a Snapshot or an `EntityMap`, a
//! notification's `notifiedAt`, a `Content-Range` bound, an aggregation period
//! a peer answers with. The rules are the clause's and nothing else: the form
//! `YYYY-MM-DDThh:mm:ss[.ffffff]Z` read by [`crate::time::lexical`], the
//! literal `Z` and never an offset (`+00:00` included), at most six fractional
//! digits and never a truncated seventh, a comma accepted for the decimal point.
//!
//! An instant the broker produces itself is brought to the same resolution by
//! [`NgsiLdDateTime::normalise`], which truncates rather than refuses because
//! no client wrote the extra digits.
//!
//! PostgreSQL `TIMESTAMPTZ` stores from `4713-01-01 BC`; the four-digit year of
//! the clause cannot name anything earlier than year 0, but an instant the
//! broker computes can, and is refused with [`NgsiLdDateTimeError::OutOfRange`]
//! rather than clamped.

use crate::time::{
    lexical::{self, DateComponents, LexicalError, TimeComponents},
    rendering::instant_text,
    resolution::to_microsecond,
};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use derive_more::{AsRef, Deref, Display};
use serde::Deserialize;
use std::str::FromStr;
use thiserror::Error;

/// The proleptic Gregorian calendar date of Julian day zero, as `(year, month, day)`.
///
/// This is `4713-01-01 BC` in the Julian calendar, the floor of the PostgreSQL
/// `TIMESTAMPTZ` domain. No ceiling is needed: `chrono`'s year ceiling of
/// 262142 is lower than PostgreSQL's 294276.
const JULIAN_DAY_ZERO: (i32, u32, u32) = (-4713, 11, 24);

/// Why a text or an instant is not an NGSI-LD `DateTime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NgsiLdDateTimeError {
    /// The text is not written in the clause 4.6.3 `DateTime` form.
    #[error("NGSI-LD DateTime value {0}")]
    Lexical(#[from] LexicalError),
    /// The text is written in the form but names no calendar instant (a
    /// thirteenth month, a thirtieth of February, a twenty-fifth hour), or the
    /// instant lies outside the range the broker stores.
    #[error("NGSI-LD DateTime value names no instant the broker can store")]
    OutOfRange,
}

/// An NGSI-LD `DateTime`: a UTC instant at microsecond resolution, inside the
/// storable range.
///
/// [`Display`] spells it as every instant the broker emits is spelled
/// ([`instant_text`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Display, Deserialize)]
#[display("{}", instant_text(*_0))]
#[serde(try_from = "String")]
pub struct NgsiLdDateTime(DateTime<Utc>);

impl NgsiLdDateTime {
    /// The instant.
    #[must_use]
    pub const fn get(self) -> DateTime<Utc> {
        self.0
    }

    /// The instant the components of a written `DateTime` name.
    ///
    /// # Errors
    ///
    /// Returns [`NgsiLdDateTimeError::OutOfRange`] when the components name no
    /// calendar instant, or one the broker cannot store.
    pub(crate) fn from_components(date: DateComponents, time: TimeComponents) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        let day = NaiveDate::from_ymd_opt(date.year, date.month, date.day).ok_or(NgsiLdDateTimeError::OutOfRange)?;
        let clock = NaiveTime::from_hms_micro_opt(time.hour, time.minute, time.second, time.microsecond).ok_or(NgsiLdDateTimeError::OutOfRange)?;
        NgsiLdDateTime::normalise(day.and_time(clock).and_utc())
    }

    /// Bring an instant the broker produced itself to microsecond resolution.
    ///
    /// Not reachable from text: [`FromStr`] refuses a seventh fractional digit
    /// rather than discarding it.
    ///
    /// # Errors
    ///
    /// Returns [`NgsiLdDateTimeError::OutOfRange`] when the instant precedes the
    /// storable floor.
    pub fn normalise(instant: DateTime<Utc>) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        let floor = storable_floor().ok_or(NgsiLdDateTimeError::OutOfRange)?;
        let truncated = to_microsecond(instant);
        if truncated < floor {
            return Err(NgsiLdDateTimeError::OutOfRange);
        }
        Ok(NgsiLdDateTime(truncated))
    }
}

// Validating parser: the clause 4.6.3 grammar and the calendar check are steps
// no derive expresses, and `derive_more::FromStr` would delegate to `chrono`'s
// own RFC 3339 reader, which admits offsets, lowercase designators and
// nanoseconds.
impl FromStr for NgsiLdDateTime {
    type Err = NgsiLdDateTimeError;

    fn from_str(text: &str) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        let (date, time) = lexical::date_time(text)?;
        NgsiLdDateTime::from_components(date, time)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter is what `#[serde(try_from = "String")]` requires.
impl TryFrom<String> for NgsiLdDateTime {
    type Error = NgsiLdDateTimeError;

    fn try_from(text: String) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        text.parse()
    }
}

/// The earliest instant PostgreSQL `TIMESTAMPTZ` stores.
fn storable_floor() -> Option<DateTime<Utc>> {
    let (year, month, day) = JULIAN_DAY_ZERO;
    Some(NaiveDate::from_ymd_opt(year, month, day)?.and_time(NaiveTime::MIN).and_utc())
}

#[cfg(test)]
mod tests {
    use crate::time::{
        date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
        lexical::LexicalError,
    };
    use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Timelike, Utc};

    /// Midnight UTC on a calendar date the clause 4.6.3 form cannot spell.
    fn utc_at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(year, month, day)
            .expect("the test names a valid calendar date")
            .and_time(NaiveTime::MIN)
            .and_utc()
    }

    fn refusal(text: &str) -> NgsiLdDateTimeError {
        text.parse::<NgsiLdDateTime>().expect_err("the text is refused")
    }

    #[test]
    fn six_fractional_digits_are_accepted_at_the_microsecond() {
        let parsed: NgsiLdDateTime = "2026-07-25T10:00:00.123456Z".parse().expect("six digits");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 123_456_000);
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00.123456Z");
    }

    /// Clause 4.6.3 caps the fraction at "a maximum of six" digits; a seventh is
    /// refused, never rounded or truncated into a different instant.
    #[test]
    fn a_seventh_fractional_digit_is_refused() {
        for text in ["2026-07-25T10:00:00.1234569Z", "1970-01-01T00:00:00.999999999Z", "1969-12-31T23:59:59.0000005Z"] {
            pretty_assertions::assert_eq!(refusal(text), NgsiLdDateTimeError::Lexical(LexicalError::TooManyFractionalDigits), "{text}");
        }
    }

    /// Clause 4.6.3: the trailing component "shall always be equal to the
    /// character 'Z'"; `+00:00` names the same instant and is still refused.
    #[test]
    fn an_offset_or_a_lowercase_designator_is_refused() {
        for text in ["2026-07-25T12:00:00.5+02:00", "2026-07-25T12:00:00+00:00", "2026-07-25T12:00:00z"] {
            let error = refusal(text);
            pretty_assertions::assert_eq!(error, NgsiLdDateTimeError::Lexical(LexicalError::ZoneDesignator), "{text}");
            assert!(error.to_string().contains("literal character 'Z'"), "{error}");
        }
    }

    /// Clause 4.6.3: "reduced representations are not permitted".
    #[test]
    fn a_reduced_representation_is_refused() {
        for text in ["2026-07-25T10:00Z", "2026-07-25T10:00:00", "2026-07-25", "not-a-timestamp"] {
            pretty_assertions::assert_eq!(refusal(text), NgsiLdDateTimeError::Lexical(LexicalError::Malformed), "{text}");
        }
    }

    /// Clause 4.6.3: "In requests, also a comma instead of a decimal point may
    /// be used as separator for compatibility reasons."
    #[test]
    fn a_comma_separator_names_the_same_instant_and_is_spelled_with_a_point() {
        let comma: NgsiLdDateTime = "2026-07-25T10:00:00,123456Z".parse().expect("a comma separator");
        let point: NgsiLdDateTime = "2026-07-25T10:00:00.123456Z".parse().expect("a decimal point");
        pretty_assertions::assert_eq!(comma, point);
        pretty_assertions::assert_eq!(comma.to_string(), "2026-07-25T10:00:00.123456Z");
    }

    /// A malformed value is a client syntax error; one written in the form that
    /// names no instant is a well-formed value the broker cannot hold.
    #[test]
    fn a_field_outside_its_calendar_range_is_out_of_range() {
        for text in [
            "2026-13-01T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T12:60:00Z",
            "2026-01-01T12:00:60Z",
        ] {
            pretty_assertions::assert_eq!(refusal(text), NgsiLdDateTimeError::OutOfRange, "{text}");
        }
    }

    #[test]
    fn year_zero_is_the_earliest_spellable_year() {
        let parsed: NgsiLdDateTime = "0000-01-01T00:00:00Z".parse().expect("year zero is above the storable floor");
        pretty_assertions::assert_eq!(parsed.get(), utc_at(0, 1, 1));
    }

    #[test]
    fn normalising_truncates_a_broker_instant_to_the_microsecond() {
        let instant = utc_at(2026, 7, 25) + TimeDelta::nanoseconds(123_456_789);
        pretty_assertions::assert_eq!(NgsiLdDateTime::normalise(instant).expect("storable").get().nanosecond(), 123_456_000);
    }

    #[test]
    fn normalising_an_already_normalised_value_changes_nothing() {
        let once: NgsiLdDateTime = "2026-07-25T10:00:00.123456Z".parse().expect("a microsecond timestamp");
        pretty_assertions::assert_eq!(NgsiLdDateTime::normalise(once.get()), Ok(once));
    }

    #[test]
    fn the_storable_floor_is_julian_day_zero() {
        pretty_assertions::assert_eq!(
            NgsiLdDateTime::normalise(utc_at(-4713, 11, 24)).map(NgsiLdDateTime::get),
            Ok(utc_at(-4713, 11, 24))
        );
        pretty_assertions::assert_eq!(NgsiLdDateTime::normalise(utc_at(-4713, 11, 23)), Err(NgsiLdDateTimeError::OutOfRange));
    }

    #[test]
    fn an_owned_string_parses_through_the_try_from_adapter() {
        let parsed = NgsiLdDateTime::try_from("2026-07-25T10:00:00Z".to_owned()).expect("an owned timestamp");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00Z");
    }

    #[test]
    fn a_json_string_deserializes_under_the_same_rules() {
        let parsed: NgsiLdDateTime = serde_json::from_str(r#""2026-07-25T10:00:00,5Z""#).expect("a JSON DateTime");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00.500Z");
        assert!(serde_json::from_str::<NgsiLdDateTime>(r#""2026-07-25T10:00:00+00:00""#).is_err());
        assert!(serde_json::from_str::<NgsiLdDateTime>("12").is_err());
    }
}
