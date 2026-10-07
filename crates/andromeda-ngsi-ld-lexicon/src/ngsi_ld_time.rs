//! The NGSI-LD `Time` lexical form in the one rendering the broker stores, orders and compares (ETSI GS CIM 009 clause 4.6.3).
//!
//! Clause 4.6.3 defines `Time` as the format `hh:mm:ssZ`, and states that "the
//! string shall not contain expressions of the difference between local time and
//! UTC", so the literal `Z` is the only zone designator a value may carry. What
//! remains variable is the fractional part: it may be absent, or carry one to six
//! digits, written with a decimal point or -- as a request-side compatibility
//! form the same clause allows -- with a comma. A broker that stores those
//! spellings verbatim compares them as text, and text comparison over them is
//! not chronological: `12:00:00.5Z` sorts before `12:00:00Z` lexically while the
//! two are the other way round in time.
//!
//! [`NgsiLdTime`] removes the ambiguity before storage. Every accepted value is
//! resolved to UTC and rendered in one fixed-width canonical form,
//! `HH:MM:SS.ffffffZ` — always six fractional digits, always `Z`. Fixed width
//! plus a fixed offset is what makes byte-wise (`COLLATE "C"`) lexical order over
//! the stored text identical to chronological order, which is what lets clause
//! 4.9's `<`, `>`, `<=` and `>=` be answered by a plain string comparison and by
//! the B-tree index over it.
//!
//! Resolution matches [`crate::ngsi_ld_date_time::NgsiLdDateTime`]: sub-microsecond
//! digits are truncated, not rounded, so the stored time of day is never later
//! than the one supplied and normalising an already-normalised value changes
//! nothing.

use crate::fractional_seconds::{FractionalSecondsError, accept_comma_decimal_separator, reject_excess_fractional_digits};
use chrono::{DateTime, NaiveTime, Timelike};
use derive_more::{AsRef, Deref, Display};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// The number of nanoseconds in one microsecond, the resolution everything below truncates to.
const NANOSECONDS_PER_MICROSECOND: u32 = 1_000;

/// The date a bare `full-time` is anchored to while its offset is resolved.
///
/// Discarded immediately afterwards. Any date would do: an offset shifts every
/// date by the same amount, so the resolved time of day does not depend on it.
const TIME_ANCHOR_DATE: &str = "1970-01-01";

/// The canonical rendering: fixed-width, always six fractional digits, always `Z`.
///
/// The width is the point. A variable-width rendering — seconds sometimes
/// carrying a fractional part and sometimes not — orders `12:00:00.5Z` before
/// `12:00:00Z` lexically while the two are the other way round chronologically.
const CANONICAL_FORMAT: &str = "%H:%M:%S%.6fZ";

/// Errors raised when a `Time` value cannot be accepted in canonical form.
#[derive(Debug, Error)]
pub enum NgsiLdTimeError {
    /// The text is not an RFC 3339 `full-time`.
    #[error("NGSI-LD Time value is not an RFC 3339 full-time: {0}")]
    Malformed(#[source] chrono::ParseError),
    /// The text's fractional-seconds part breaks the clause 4.6.3 rule.
    #[error("NGSI-LD Time {0}")]
    FractionalSeconds(#[from] FractionalSecondsError),
    /// The time of day cannot be held at microsecond resolution.
    #[error("NGSI-LD Time value cannot be held at microsecond resolution")]
    OutOfRange,
    /// The text's trailing zone designator is not the literal `Z` character.
    #[error("NGSI-LD Time value's trailing zone designator is not the literal character 'Z'")]
    ZoneDesignator,
}

/// An NGSI-LD `Time` resolved to UTC and truncated to microsecond resolution.
///
/// A value of this type renders through [`Display`] as `HH:MM:SS.ffffffZ`, the
/// single form the broker stores. Two values compare through [`Ord`] in
/// chronological order, and their canonical renderings compare byte-wise in the
/// same order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Display, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
#[display("{}", _0.format(CANONICAL_FORMAT))]
pub struct NgsiLdTime(NaiveTime);

impl NgsiLdTime {
    /// The normalised time of day, in UTC.
    #[must_use]
    pub const fn get(self) -> NaiveTime {
        self.0
    }

    /// Normalise an already-typed UTC time of day to microsecond resolution.
    ///
    /// This is the same policy the parser applies, reached from a `chrono` value
    /// rather than from lexical text. The input is taken to be UTC already: a
    /// [`NaiveTime`] carries no offset, so there is nothing here to resolve.
    ///
    /// # Errors
    ///
    /// Returns [`NgsiLdTimeError::OutOfRange`] when discarding the
    /// sub-microsecond remainder would not yield a representable time of day.
    pub fn normalise(time: NaiveTime) -> Result<NgsiLdTime, NgsiLdTimeError> {
        let nanoseconds = time.nanosecond();
        time.with_nanosecond(nanoseconds - nanoseconds % NANOSECONDS_PER_MICROSECOND)
            .map(NgsiLdTime)
            .ok_or(NgsiLdTimeError::OutOfRange)
    }
}

// Validating parser: the RFC 3339 `full-time` grammar, the resolution to UTC and
// the microsecond truncation policy are three steps no derive expresses, and the
// `derive_more::FromStr` a newtype would otherwise get delegates to `NaiveTime`'s
// own parser, which requires no offset and applies none of them.
impl FromStr for NgsiLdTime {
    type Err = NgsiLdTimeError;

    fn from_str(value: &str) -> Result<NgsiLdTime, NgsiLdTimeError> {
        let candidate = accept_comma_decimal_separator(value)?;
        reject_excess_fractional_digits(&candidate)?;
        // RFC 3339 defines `full-time` as `partial-time time-offset`, so the
        // offset is mandatory: without one the time of day would depend on
        // whichever zone the reader happened to assume.
        let anchored = format!("{TIME_ANCHOR_DATE}T{candidate}");
        let resolved = DateTime::parse_from_rfc3339(&anchored).map_err(NgsiLdTimeError::Malformed)?;
        // Clause 4.6.3 gives the form `hh:mm:ssZ` and forbids "expressions of
        // the difference between local time and UTC", so the literal `Z` is the
        // only designator a value may end in. A numeric offset is the expression
        // the clause forbids and RFC 3339's lowercase `z` is not the character
        // it names; both are refused here, and the refusal names the designator
        // because the second carries no offset at all.
        if !candidate.ends_with('Z') {
            return Err(NgsiLdTimeError::ZoneDesignator);
        }
        NgsiLdTime::normalise(resolved.naive_utc().time())
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this adapter
// exists for callers that already own the text.
impl TryFrom<String> for NgsiLdTime {
    type Error = NgsiLdTimeError;

    fn try_from(value: String) -> Result<NgsiLdTime, NgsiLdTimeError> {
        value.parse()
    }
}

// `derive_more::Into` would yield `Into<NaiveTime>`, the inner type. What
// `#[serde(into = "String")]` needs is the canonical text, which no derive
// produces from a `chrono` value.
impl From<NgsiLdTime> for String {
    fn from(time: NgsiLdTime) -> String {
        time.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        fractional_seconds::FractionalSecondsError,
        ngsi_ld_time::{NgsiLdTime, NgsiLdTimeError},
    };
    use chrono::{NaiveTime, Timelike};

    #[test]
    fn a_whole_second_time_renders_with_six_fractional_digits_and_a_zulu_offset() {
        let parsed: NgsiLdTime = "12:00:00Z".parse().expect("a full-time");
        pretty_assertions::assert_eq!(parsed.to_string(), "12:00:00.000000Z");
    }

    /// Clause 4.6.3, `Time`: "The string shall not contain expressions of the
    /// difference between local time and UTC. All representations shall be
    /// interpreted as being expressed in UTC." A numeric offset is exactly such
    /// an expression, so `13:00:00+01:00` is refused rather than resolved to
    /// `12:00:00Z`, which would let a forbidden spelling round-trip as a valid
    /// value.
    #[test]
    fn a_numeric_offset_is_refused_rather_than_resolved() {
        for value in ["13:00:00+01:00", "23:30:00-05:00", "12:00:00+00:00"] {
            let error = value.parse::<NgsiLdTime>().expect_err("a numeric offset is forbidden");
            assert!(matches!(error, NgsiLdTimeError::ZoneDesignator), "{value}: got {error:?}");
        }
    }

    /// Clause 4.6.3 gives the form `hh:mm:ssZ`, and RFC 3339's lowercase `z`
    /// names the same zone without being the character the clause writes. It
    /// carries no expression of a difference from UTC at all, so the refusal
    /// names the designator rather than an offset.
    #[test]
    fn a_lowercase_z_designator_is_refused_as_the_designator_it_is_not() {
        let error = "12:00:00z"
            .parse::<NgsiLdTime>()
            .expect_err("lowercase z is not the character the clause writes");
        assert!(matches!(error, NgsiLdTimeError::ZoneDesignator), "got {error:?}");
        assert!(error.to_string().contains("literal character 'Z'"), "{error}");
    }

    /// Clause 4.6.3, `Time`: "In requests, also a comma instead of a decimal
    /// point may be used as separator for compatibility reasons."
    #[test]
    fn a_comma_decimal_separator_is_accepted_as_the_compatibility_form() {
        let comma: NgsiLdTime = "12:00:00,5Z".parse().expect("the comma compatibility form");
        let dot: NgsiLdTime = "12:00:00.5Z".parse().expect("the canonical form");
        pretty_assertions::assert_eq!(comma, dot);
        pretty_assertions::assert_eq!(comma.to_string(), "12:00:00.500000Z");
    }

    /// Clause 4.6.3, `Time`: the fraction runs "one or more fractional digits,
    /// up to a maximum of six".
    #[test]
    fn a_fraction_beyond_six_digits_is_refused() {
        let error = "12:00:00.1234567Z".parse::<NgsiLdTime>().expect_err("seven digits exceeds the clause maximum");
        assert!(
            matches!(error, NgsiLdTimeError::FractionalSeconds(FractionalSecondsError::TooManyDigits)),
            "got {error:?}"
        );
    }

    #[test]
    fn a_fractional_part_is_padded_to_the_full_six_digits() {
        let parsed: NgsiLdTime = "12:00:00.5Z".parse().expect("a full-time");
        pretty_assertions::assert_eq!(parsed.to_string(), "12:00:00.500000Z");
    }

    #[test]
    fn sub_microsecond_digits_are_truncated_rather_than_rounded() {
        // The declared policy discards the remainder, so the stored time of day
        // is never later than the one supplied. Clause 4.6.3 caps the lexical
        // fraction at six digits, so sub-microsecond input reaches this type as
        // an already-typed `NaiveTime` rather than as text.
        let nanosecond = NaiveTime::from_hms_nano_opt(12, 0, 0, 123_456_900).expect("a time of day");
        let truncated = NgsiLdTime::normalise(nanosecond).expect("a truncatable time of day");
        pretty_assertions::assert_eq!(truncated.get().nanosecond(), 123_456_000);
        pretty_assertions::assert_eq!(truncated.to_string(), "12:00:00.123456Z");
    }

    #[test]
    fn normalising_an_already_normalised_value_changes_nothing() {
        let once: NgsiLdTime = "12:00:00.123456Z".parse().expect("a full-time");
        let twice = NgsiLdTime::normalise(once.get()).expect("an already-truncated time of day");
        pretty_assertions::assert_eq!(once, twice);
    }

    #[test]
    fn a_time_without_an_offset_is_malformed() {
        // RFC 3339 `full-time` requires an offset; accepting a bare
        // `partial-time` would make the stored value depend on the reader's zone.
        let error = "12:00:00".parse::<NgsiLdTime>().expect_err("the offset is missing");
        assert!(matches!(error, NgsiLdTimeError::Malformed { .. }), "got {error:?}");
    }

    #[test]
    fn text_that_is_not_a_time_of_day_is_malformed() {
        for value in ["noon", "25:00:00Z", "12:60:00Z", "2024-01-01T12:00:00Z"] {
            let error = value.parse::<NgsiLdTime>().expect_err("not a full-time");
            assert!(matches!(error, NgsiLdTimeError::Malformed { .. }), "{value}: got {error:?}");
        }
    }

    /// The property the canonical form exists for: byte-wise order over the
    /// rendered text is chronological order, which is what clause 4.9's ordering
    /// operators are answered by once the value is stored as text.
    #[test]
    fn byte_wise_order_over_the_canonical_form_is_chronological_order() {
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
        let parsed: Vec<NgsiLdTime> = ordered.iter().map(|value| value.parse().expect("a full-time")).collect();
        for window in parsed.windows(2) {
            let (earlier, later) = (window[0], window[1]);
            assert!(earlier < later, "{earlier} must precede {later} chronologically");
            assert!(earlier.to_string() < later.to_string(), "{earlier} must precede {later} byte-wise as well");
        }
    }

    #[test]
    fn an_owned_string_parses_through_the_try_from_adapter() {
        let parsed = NgsiLdTime::try_from("12:00:00Z".to_owned()).expect("an owned full-time");
        pretty_assertions::assert_eq!(parsed.to_string(), "12:00:00.000000Z");
    }

    #[test]
    fn a_typed_time_of_day_normalises_without_going_through_text() {
        let time = NaiveTime::from_hms_nano_opt(12, 0, 0, 123_456_789).expect("a valid time of day");
        pretty_assertions::assert_eq!(NgsiLdTime::normalise(time).expect("truncatable").to_string(), "12:00:00.123456Z");
    }
}
