//! The NGSI-LD `DateTime` lexical form at the broker's declared precision (ETSI GS CIM 009 clause 4.6.3).
//!
//! Clause 4.6.3 defines `DateTime` as an IETF RFC 3339 timestamp, which admits
//! an unbounded number of fractional-second digits and an unbounded year. Neither
//! is storable: PostgreSQL `TIMESTAMPTZ` resolves to microseconds and spans
//! `4713-01-01 BC` to `294276 AD`, and it rounds finer input on ingestion without
//! the broker ever deciding that.
//!
//! [`NgsiLdDateTime`] is where the broker decides instead. Microsecond resolution
//! is Andromeda's declared temporal precision: a value carrying finer fractional
//! digits is truncated here, in Rust, under that documented policy, so the value
//! the broker stores is the value it renders and compares. Truncation discards the
//! sub-microsecond remainder rather than rounding to the nearest microsecond, so
//! the normalised instant is never later than the instant supplied and normalising
//! an already-normalised value changes nothing. An instant outside the storable
//! range is refused with [`NgsiLdDateTimeError::OutOfRange`] rather than being
//! clamped.
//!
//! The policy and its consequences are set out in `docs/timestamp-precision.md`.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, SecondsFormat, Timelike, Utc, format::ParseErrorKind};
use derive_more::{AsRef, Deref, Display};
use std::str::FromStr;
use thiserror::Error;

/// The number of nanoseconds in one microsecond, the resolution everything below truncates to.
const NANOSECONDS_PER_MICROSECOND: u32 = 1_000;

/// The proleptic Gregorian calendar date of Julian day zero, as `(year, month, day)`.
///
/// This is `4713-01-01 BC` in the Julian calendar, which is the floor of the
/// PostgreSQL `TIMESTAMPTZ` domain. The ceiling is not stated here because
/// `chrono`'s own year ceiling of 262142 is the lower of the two, so a value
/// beyond it never survives parsing.
const JULIAN_DAY_ZERO: (i32, u32, u32) = (-4713, 11, 24);

/// Errors raised when a `DateTime` value cannot be accepted at microsecond resolution.
#[derive(Debug, Error)]
pub enum NgsiLdDateTimeError {
    /// The text is not an RFC 3339 timestamp.
    #[error("NGSI-LD DateTime value is not an RFC 3339 timestamp: {value}")]
    Malformed {
        /// The rejected text.
        value: String,
    },
    /// The text names an instant outside the range the broker can store.
    #[error("NGSI-LD DateTime value names an instant outside the storable range: {value}")]
    OutOfRange {
        /// The rejected text.
        value: String,
    },
}

/// An NGSI-LD `DateTime` normalised to the broker's microsecond precision.
///
/// A value of this type is known to be storable and to carry no fractional digits
/// finer than a microsecond, so persisting it and rendering it yield the same
/// instant. [`Display`] renders it as RFC 3339 in UTC with six fractional digits,
/// which is the lexical form at exactly the resolution the value holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Display)]
#[display("{}", _0.to_rfc3339_opts(SecondsFormat::Micros, true))]
pub struct NgsiLdDateTime(DateTime<Utc>);

impl NgsiLdDateTime {
    /// The normalised instant.
    pub const fn get(self) -> DateTime<Utc> {
        self.0
    }

    /// Normalise an already-typed instant to microsecond resolution.
    ///
    /// This is the same policy the parser applies, reached from a `chrono` value
    /// rather than from lexical text.
    ///
    /// # Errors
    ///
    /// Returns [`NgsiLdDateTimeError::OutOfRange`] when the instant precedes the
    /// storable floor, or when discarding its sub-microsecond remainder would not
    /// yield a representable instant.
    pub fn normalise(instant: DateTime<Utc>) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        let Some(truncated) = truncate_to_microseconds(instant) else {
            return Err(NgsiLdDateTimeError::OutOfRange { value: instant.to_rfc3339() });
        };
        let Some(floor) = storable_floor() else {
            // `JULIAN_DAY_ZERO` is a fixed valid calendar date, so this branch is
            // unreachable; refusing rather than skipping the check keeps an
            // unconstructible floor from silently widening the accepted range.
            return Err(NgsiLdDateTimeError::OutOfRange { value: instant.to_rfc3339() });
        };
        if truncated < floor {
            return Err(NgsiLdDateTimeError::OutOfRange { value: instant.to_rfc3339() });
        }
        Ok(NgsiLdDateTime(truncated))
    }
}

// Validating parser: the RFC 3339 grammar, the microsecond truncation policy and
// the storable-range check are three steps no derive expresses, and the
// `derive_more::FromStr` a newtype would otherwise get delegates to the inner
// type's parser, which applies none of them.
impl FromStr for NgsiLdDateTime {
    type Err = NgsiLdDateTimeError;

    fn from_str(value: &str) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        // `chrono` refuses a year beyond its own ceiling as a parse failure, so a
        // lexically valid timestamp that overflows is reported as out of range
        // rather than as malformed text.
        let parsed = DateTime::parse_from_rfc3339(value).map_err(|error| {
            if error.kind() == ParseErrorKind::OutOfRange {
                NgsiLdDateTimeError::OutOfRange { value: value.to_string() }
            } else {
                NgsiLdDateTimeError::Malformed { value: value.to_string() }
            }
        })?;
        NgsiLdDateTime::normalise(parsed.with_timezone(&Utc)).map_err(|_| NgsiLdDateTimeError::OutOfRange { value: value.to_string() })
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this adapter
// exists for callers that already own the text.
impl TryFrom<String> for NgsiLdDateTime {
    type Error = NgsiLdDateTimeError;

    fn try_from(value: String) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        value.parse()
    }
}

/// Discard the sub-microsecond remainder of an instant.
///
/// The nanosecond field `chrono` exposes is always non-negative, so removing the
/// remainder always yields an instant no later than the one supplied, on both
/// sides of the epoch. Values in the 1..2 second range of the field represent a
/// leap second and stay in that range, which is why the remainder is removed in
/// place rather than by subtracting a duration.
fn truncate_to_microseconds(instant: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let nanoseconds = instant.nanosecond();
    instant.with_nanosecond(nanoseconds - nanoseconds % NANOSECONDS_PER_MICROSECOND)
}

/// The earliest instant PostgreSQL `TIMESTAMPTZ` stores.
fn storable_floor() -> Option<DateTime<Utc>> {
    let (year, month, day) = JULIAN_DAY_ZERO;
    Some(NaiveDate::from_ymd_opt(year, month, day)?.and_time(NaiveTime::MIN).and_utc())
}

/// The calendar year of the storable floor, for callers reporting the accepted range.
pub const fn earliest_storable_year() -> i32 {
    JULIAN_DAY_ZERO.0
}

/// The calendar year of the storable ceiling, which is `chrono`'s own year ceiling.
pub fn latest_storable_year() -> i32 {
    DateTime::<Utc>::MAX_UTC.year()
}

#[cfg(test)]
mod tests {
    use crate::ngsi_ld_date_time::{NgsiLdDateTime, NgsiLdDateTimeError, earliest_storable_year, latest_storable_year};
    use chrono::{DateTime, NaiveDate, NaiveTime, Timelike, Utc};

    /// Midnight UTC on a calendar date the RFC 3339 grammar cannot spell.
    fn utc_at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(year, month, day)
            .expect("the test names a valid calendar date")
            .and_time(NaiveTime::MIN)
            .and_utc()
    }

    #[test]
    fn a_microsecond_precision_timestamp_round_trips_unchanged() {
        let parsed: NgsiLdDateTime = "2026-07-25T10:00:00.123456Z".parse().expect("a microsecond timestamp");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00.123456Z");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 123_456_000);
    }

    #[test]
    fn sub_microsecond_digits_are_truncated_rather_than_rounded() {
        // Rounding would carry .1234569 up to .123457; the declared policy is to
        // discard the remainder, so the stored instant is never later than the
        // one supplied.
        let parsed: NgsiLdDateTime = "2026-07-25T10:00:00.1234569Z".parse().expect("a sub-microsecond timestamp");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 123_456_000);
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00.123456Z");
    }

    #[test]
    fn a_full_nanosecond_timestamp_loses_exactly_its_last_three_digits() {
        let parsed: NgsiLdDateTime = "1970-01-01T00:00:00.999999999Z".parse().expect("a nanosecond timestamp");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 999_999_000);
    }

    #[test]
    fn truncation_before_the_epoch_also_moves_the_instant_earlier() {
        // The nanosecond field is non-negative on both sides of the epoch, so a
        // pre-epoch value truncates in the same direction as a later one.
        let parsed: NgsiLdDateTime = "1969-12-31T23:59:59.0000005Z".parse().expect("a pre-epoch timestamp");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 0);
        pretty_assertions::assert_eq!(parsed.to_string(), "1969-12-31T23:59:59.000000Z");
    }

    #[test]
    fn normalising_an_already_normalised_value_changes_nothing() {
        let once: NgsiLdDateTime = "2026-07-25T10:00:00.1234569Z".parse().expect("a sub-microsecond timestamp");
        let twice = NgsiLdDateTime::normalise(once.get()).expect("an already-truncated instant stays storable");
        pretty_assertions::assert_eq!(once, twice);
    }

    #[test]
    fn a_calendar_field_outside_its_permitted_range_is_out_of_range_not_malformed() {
        // The distinction matters to the caller: a malformed value is a client
        // syntax error, an out-of-range one is a well-formed value the broker
        // cannot store.
        let error = "2026-13-01T00:00:00Z".parse::<NgsiLdDateTime>().expect_err("month 13 is out of range");
        assert!(matches!(error, NgsiLdDateTimeError::OutOfRange { .. }), "got {error:?}");
    }

    #[test]
    fn an_instant_before_the_storable_floor_is_refused() {
        let before_floor = utc_at(-262_143, 1, 1);
        let error = NgsiLdDateTime::normalise(before_floor).expect_err("the instant precedes the storable floor");
        assert!(matches!(error, NgsiLdDateTimeError::OutOfRange { .. }), "got {error:?}");
    }

    #[test]
    fn the_storable_floor_itself_is_accepted() {
        let floor = NgsiLdDateTime::normalise(utc_at(-4713, 11, 24)).expect("Julian day zero is storable");
        pretty_assertions::assert_eq!(floor.get().date_naive().to_string(), "-4713-11-24");
    }

    #[test]
    fn the_day_before_the_storable_floor_is_refused() {
        let error = NgsiLdDateTime::normalise(utc_at(-4713, 11, 23)).expect_err("one day before the floor is not storable");
        assert!(matches!(error, NgsiLdDateTimeError::OutOfRange { .. }), "got {error:?}");
    }

    #[test]
    fn text_that_is_not_a_timestamp_is_malformed_rather_than_out_of_range() {
        let error = "not-a-timestamp".parse::<NgsiLdDateTime>().expect_err("the text is not RFC 3339");
        assert!(matches!(error, NgsiLdDateTimeError::Malformed { .. }), "got {error:?}");
    }

    #[test]
    fn a_timestamp_without_an_offset_is_malformed() {
        // RFC 3339 requires an offset; accepting a naive form would make the
        // stored instant depend on the broker's own zone.
        let error = "2026-07-25T10:00:00".parse::<NgsiLdDateTime>().expect_err("the offset is missing");
        assert!(matches!(error, NgsiLdDateTimeError::Malformed { .. }), "got {error:?}");
    }

    #[test]
    fn a_non_utc_offset_is_normalised_to_utc() {
        let parsed: NgsiLdDateTime = "2026-07-25T12:00:00.5+02:00".parse().expect("an offset timestamp");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00.500000Z");
    }

    #[test]
    fn the_reported_storable_years_bracket_the_accepted_range() {
        pretty_assertions::assert_eq!(earliest_storable_year(), -4713);
        assert!(latest_storable_year() > 262_000, "got {}", latest_storable_year());
    }

    #[test]
    fn an_owned_string_parses_through_the_try_from_adapter() {
        let parsed = NgsiLdDateTime::try_from("2026-07-25T10:00:00Z".to_string()).expect("an owned timestamp");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00.000000Z");
    }
}
