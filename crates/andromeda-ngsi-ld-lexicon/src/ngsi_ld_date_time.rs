//! The NGSI-LD `DateTime` lexical form at the broker's declared precision (ETSI GS CIM 009 clause 4.6.3).
//!
//! Clause 4.6.3 defines `DateTime` as an IETF RFC 3339 timestamp restricted to
//! UTC: the trailing zone designator "shall always be equal to the character
//! 'Z'", and the fractional-second part carries "up to a maximum of six"
//! digits. [`NgsiLdDateTime`] enforces both restrictions on parsed input
//! rather than silently reinterpreting it: a non-`Z` offset and more than six
//! fractional digits are each refused with their own typed error instead of
//! being normalised or truncated. Clause 4.6.3 also carries a compatibility
//! note accepting a comma in place of the dot as the decimal separator; a
//! client-supplied comma is accepted and rewritten to the canonical dot
//! before parsing, so the value the broker stores and renders is always
//! dot-separated.
//!
//! PostgreSQL `TIMESTAMPTZ` resolves to microseconds and spans `4713-01-01 BC`
//! to `294276 AD`, narrower than clause 4.6.3's own unbounded year range. An
//! instant outside that storable range is refused with
//! [`NgsiLdDateTimeError::OutOfRange`] rather than clamped.
//!
//! [`NgsiLdDateTime::normalise`] additionally truncates a Rust-native instant
//! — such as the broker's own write clock — to microsecond resolution. That
//! path never sees client-supplied lexical text, so it is not a relaxation of
//! the six-digit limit parsing enforces above; it exists because a value the
//! broker itself mints can carry nanosecond resolution that the storage
//! column cannot hold.
//!
//! The policy and its consequences are set out in `docs/timestamp-precision.md`.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, SecondsFormat, Timelike, Utc, format::ParseErrorKind};
use derive_more::{AsRef, Deref, Display};
use std::{borrow::Cow, str::FromStr};
use thiserror::Error;

/// The number of nanoseconds in one microsecond, the resolution [`NgsiLdDateTime::normalise`] truncates to.
const NANOSECONDS_PER_MICROSECOND: u32 = 1_000;

/// The maximum number of fractional-second digits clause 4.6.3 admits.
const MAX_FRACTIONAL_DIGITS: usize = 6;

/// The proleptic Gregorian calendar date of Julian day zero, as `(year, month, day)`.
///
/// This is `4713-01-01 BC` in the Julian calendar, which is the floor of the
/// PostgreSQL `TIMESTAMPTZ` domain. The ceiling is not stated here because
/// `chrono`'s own year ceiling of 262142 is the lower of the two, so a value
/// beyond it never survives parsing.
const JULIAN_DAY_ZERO: (i32, u32, u32) = (-4713, 11, 24);

/// Errors raised when a `DateTime` value cannot be accepted under the clause 4.6.3 policy.
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
    /// The text carries a zone designator other than the literal `Z` character.
    #[error("NGSI-LD DateTime value does not use the mandatory 'Z' UTC designator: {value}")]
    NonUtcOffset {
        /// The rejected text.
        value: String,
    },
    /// The text carries more than six fractional-second digits.
    #[error("NGSI-LD DateTime value carries more than six fractional-second digits: {value}")]
    TooManyFractionalDigits {
        /// The rejected text.
        value: String,
    },
}

/// An NGSI-LD `DateTime` accepted under the broker's strict clause 4.6.3 policy.
///
/// A value of this type is known to be storable, to carry the literal `Z` UTC
/// designator, and to carry no more than six fractional digits, so persisting
/// it and rendering it yield the same instant. [`Display`] renders it as RFC
/// 3339 in UTC with six fractional digits, which is the canonical lexical
/// form at exactly the resolution the value holds.
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
    /// This truncates a Rust-native instant — one the broker minted itself
    /// rather than parsed from client text — to the storable resolution. It
    /// is not reachable from [`FromStr`], which refuses rather than truncates
    /// finer client-supplied precision.
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

// Validating parser: the RFC 3339 grammar, the comma compatibility rewrite,
// the mandatory-`Z` and six-digit precision checks, and the storable-range
// check are steps no derive expresses, and the `derive_more::FromStr` a
// newtype would otherwise get delegates to the inner type's parser, which
// applies none of them.
impl FromStr for NgsiLdDateTime {
    type Err = NgsiLdDateTimeError;

    fn from_str(value: &str) -> Result<NgsiLdDateTime, NgsiLdDateTimeError> {
        let candidate = accept_comma_decimal_separator(value)?;
        if fractional_digit_count(&candidate).is_some_and(|count| count > MAX_FRACTIONAL_DIGITS) {
            return Err(NgsiLdDateTimeError::TooManyFractionalDigits { value: value.to_string() });
        }
        // `chrono` refuses a year beyond its own ceiling as a parse failure, so a
        // lexically valid timestamp that overflows is reported as out of range
        // rather than as malformed text.
        let parsed = DateTime::parse_from_rfc3339(&candidate).map_err(|error| {
            if error.kind() == ParseErrorKind::OutOfRange {
                NgsiLdDateTimeError::OutOfRange { value: value.to_string() }
            } else {
                NgsiLdDateTimeError::Malformed { value: value.to_string() }
            }
        })?;
        // Clause 4.6.3 requires the literal `Z` character; `+00:00` and every
        // other numeric offset name the same instant but are not that
        // character, so both are refused rather than accepted as synonyms.
        if !candidate.ends_with('Z') {
            return Err(NgsiLdDateTimeError::NonUtcOffset { value: value.to_string() });
        }
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

/// Rewrite a client-supplied comma decimal separator to the canonical dot.
///
/// Clause 4.6.3 carries a compatibility note accepting a comma in requests.
/// Exactly one comma is the form that note describes; text with more than one
/// comma can never be a valid timestamp either way, so it is reported as
/// malformed rather than silently collapsed.
fn accept_comma_decimal_separator(value: &str) -> Result<Cow<'_, str>, NgsiLdDateTimeError> {
    match value.matches(',').count() {
        0 => Ok(Cow::Borrowed(value)),
        1 => Ok(Cow::Owned(value.replacen(',', ".", 1))),
        _ => Err(NgsiLdDateTimeError::Malformed { value: value.to_string() }),
    }
}

/// Count the fractional-second digits in a dot-normalised candidate, if it declares any.
fn fractional_digit_count(candidate: &str) -> Option<usize> {
    let (_, after_dot) = candidate.split_once('.')?;
    Some(after_dot.chars().take_while(char::is_ascii_digit).count())
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
    fn seven_fractional_digits_are_rejected_rather_than_rounded() {
        // Rounding would carry .1234569 up to .123457; the strict policy is to
        // refuse the extra digit rather than reinterpret the value.
        let error = "2026-07-25T10:00:00.1234569Z"
            .parse::<NgsiLdDateTime>()
            .expect_err("seven fractional digits exceed clause 4.6.3");
        assert!(matches!(error, NgsiLdDateTimeError::TooManyFractionalDigits { .. }), "got {error:?}");
    }

    #[test]
    fn a_full_nanosecond_timestamp_is_rejected() {
        let error = "1970-01-01T00:00:00.999999999Z"
            .parse::<NgsiLdDateTime>()
            .expect_err("nine fractional digits exceed clause 4.6.3");
        assert!(matches!(error, NgsiLdDateTimeError::TooManyFractionalDigits { .. }), "got {error:?}");
    }

    #[test]
    fn seven_fractional_digits_before_the_epoch_are_also_rejected() {
        let error = "1969-12-31T23:59:59.0000005Z"
            .parse::<NgsiLdDateTime>()
            .expect_err("seven fractional digits exceed clause 4.6.3 regardless of sign");
        assert!(matches!(error, NgsiLdDateTimeError::TooManyFractionalDigits { .. }), "got {error:?}");
    }

    #[test]
    fn exactly_six_fractional_digits_is_the_accepted_maximum() {
        let parsed: NgsiLdDateTime = "2026-07-25T10:00:00.999999Z".parse().expect("six fractional digits is the declared maximum");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 999_999_000);
    }

    #[test]
    fn normalising_an_already_normalised_value_changes_nothing() {
        let once: NgsiLdDateTime = "2026-07-25T10:00:00.123456Z".parse().expect("a microsecond timestamp");
        let twice = NgsiLdDateTime::normalise(once.get()).expect("an already-normalised instant stays storable");
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
    fn a_non_utc_offset_is_rejected() {
        // Clause 4.6.3: the trailing zone "shall always be equal to the
        // character 'Z'". A numeric offset names a valid RFC 3339 instant but
        // is not that character, so it is refused rather than normalised.
        let error = "2026-07-25T12:00:00.5+02:00".parse::<NgsiLdDateTime>().expect_err("a non-Z offset is rejected");
        assert!(matches!(error, NgsiLdDateTimeError::NonUtcOffset { .. }), "got {error:?}");
    }

    #[test]
    fn a_plus_zero_offset_is_rejected_even_though_it_names_utc() {
        // `+00:00` names the same instant as `Z` but is not the literal
        // character the clause mandates.
        let error = "2026-07-25T12:00:00+00:00"
            .parse::<NgsiLdDateTime>()
            .expect_err("+00:00 is not the literal Z designator");
        assert!(matches!(error, NgsiLdDateTimeError::NonUtcOffset { .. }), "got {error:?}");
    }

    #[test]
    fn a_comma_decimal_separator_is_accepted_and_rendered_with_a_dot() {
        // Clause 4.6.3's compatibility note accepts a comma in requests; the
        // canonical rendering stays dot-separated regardless of what the
        // client sent.
        let parsed: NgsiLdDateTime = "2026-07-25T10:00:00,123456Z".parse().expect("a comma decimal separator is accepted");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-07-25T10:00:00.123456Z");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 123_456_000);
    }

    #[test]
    fn a_comma_decimal_separator_still_enforces_the_six_digit_limit() {
        let error = "2026-07-25T10:00:00,1234567Z"
            .parse::<NgsiLdDateTime>()
            .expect_err("seven digits exceed clause 4.6.3 whichever separator was used");
        assert!(matches!(error, NgsiLdDateTimeError::TooManyFractionalDigits { .. }), "got {error:?}");
    }

    #[test]
    fn more_than_one_comma_is_malformed() {
        let error = "2026-07-25T10:00:00,12,3456Z"
            .parse::<NgsiLdDateTime>()
            .expect_err("two commas cannot spell one decimal separator");
        assert!(matches!(error, NgsiLdDateTimeError::Malformed { .. }), "got {error:?}");
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
