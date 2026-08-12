//! The NGSI-LD `Time` lexical form in the one rendering the broker stores, orders and compares (ETSI GS CIM 009 clause 4.6.3).
//!
//! Clause 4.6.3 defines `Time` as an IETF RFC 3339 `full-time`, which pins a time
//! of day exactly but admits many spellings of the same one: the offset may be
//! `Z` or any numeric zone, the fractional part may be absent or carry any number
//! of digits. A broker that stores those spellings verbatim compares them as
//! text, and text comparison over them is not chronological — `12:00:00Z` sorts
//! after `11:00:00+01:00` even though the two name the same instant of the day,
//! and `09:00:00Z` sorts before `9:00:00Z`-shaped variants unpredictably.
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
    #[error("NGSI-LD Time value is not an RFC 3339 full-time: {value}")]
    Malformed {
        /// The rejected text.
        value: String,
    },
    /// The time of day cannot be held at microsecond resolution.
    #[error("NGSI-LD Time value cannot be held at microsecond resolution: {value}")]
    OutOfRange {
        /// The rejected text.
        value: String,
    },
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
            .ok_or_else(|| NgsiLdTimeError::OutOfRange {
                value: time.format(CANONICAL_FORMAT).to_string(),
            })
    }
}

// Validating parser: the RFC 3339 `full-time` grammar, the resolution to UTC and
// the microsecond truncation policy are three steps no derive expresses, and the
// `derive_more::FromStr` a newtype would otherwise get delegates to `NaiveTime`'s
// own parser, which requires no offset and applies none of them.
impl FromStr for NgsiLdTime {
    type Err = NgsiLdTimeError;

    fn from_str(value: &str) -> Result<NgsiLdTime, NgsiLdTimeError> {
        // RFC 3339 defines `full-time` as `partial-time time-offset`, so the
        // offset is mandatory: without one the time of day would depend on
        // whichever zone the reader happened to assume.
        let anchored = format!("{TIME_ANCHOR_DATE}T{value}");
        let resolved = DateTime::parse_from_rfc3339(&anchored).map_err(|_| NgsiLdTimeError::Malformed { value: value.to_owned() })?;
        NgsiLdTime::normalise(resolved.naive_utc().time()).map_err(|_| NgsiLdTimeError::OutOfRange { value: value.to_owned() })
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
    use crate::ngsi_ld_time::{NgsiLdTime, NgsiLdTimeError};
    use chrono::{NaiveTime, Timelike};

    #[test]
    fn a_whole_second_time_renders_with_six_fractional_digits_and_a_zulu_offset() {
        let parsed: NgsiLdTime = "12:00:00Z".parse().expect("a full-time");
        pretty_assertions::assert_eq!(parsed.to_string(), "12:00:00.000000Z");
    }

    #[test]
    fn a_numeric_offset_is_resolved_to_utc() {
        // Clause 4.6.3 admits any offset, and two spellings of the same time of
        // day must reach storage as one value.
        let east: NgsiLdTime = "13:00:00+01:00".parse().expect("a full-time");
        let zulu: NgsiLdTime = "12:00:00Z".parse().expect("a full-time");
        pretty_assertions::assert_eq!(east, zulu);
        pretty_assertions::assert_eq!(east.to_string(), "12:00:00.000000Z");
    }

    #[test]
    fn an_offset_that_crosses_midnight_keeps_only_the_time_of_day() {
        let parsed: NgsiLdTime = "23:30:00-05:00".parse().expect("a full-time");
        pretty_assertions::assert_eq!(parsed.to_string(), "04:30:00.000000Z");
    }

    #[test]
    fn a_fractional_part_is_padded_to_the_full_six_digits() {
        let parsed: NgsiLdTime = "12:00:00.5Z".parse().expect("a full-time");
        pretty_assertions::assert_eq!(parsed.to_string(), "12:00:00.500000Z");
    }

    #[test]
    fn sub_microsecond_digits_are_truncated_rather_than_rounded() {
        // The declared policy discards the remainder, so the stored time of day
        // is never later than the one supplied.
        let parsed: NgsiLdTime = "12:00:00.1234569Z".parse().expect("a full-time");
        pretty_assertions::assert_eq!(parsed.get().nanosecond(), 123_456_000);
        pretty_assertions::assert_eq!(parsed.to_string(), "12:00:00.123456Z");
    }

    #[test]
    fn normalising_an_already_normalised_value_changes_nothing() {
        let once: NgsiLdTime = "12:00:00.1234569Z".parse().expect("a full-time");
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
            "12:00:00+01:00",
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
