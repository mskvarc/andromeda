// SPDX-FileCopyrightText: 2014-2025 Sean McArthur
// SPDX-License-Identifier: MIT

//! Typed NGSI-LD temporal `Content-Range` response header (ETSI GS CIM 009,
//! clause 6.3.10).
//!
//! This file is derived from the `ContentRange` implementation in
//! <https://github.com/hyperium/headers>, extended for the NGSI-LD `DateTime`
//! range unit rather than byte ranges.
//! This file, including local modifications, is distributed under the MIT License.

use andromeda_ngsi_ld_lexicon::time::{
    date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
    rendering::instant_text,
};
use chrono::{DateTime, Utc};
use http::{HeaderName, HeaderValue};
use std::{fmt, iter, num::ParseIntError, str::FromStr};
use thiserror::Error;

/// The lowercase `Content-Range` field name this header is transmitted under.
static CONTENT_RANGE: HeaderName = HeaderName::from_static("content-range");

/// The range unit clause 6.3.10 mandates: `"unit" shall be equal to "DateTime"`.
///
/// This is a spec-defined token, so it keeps the specification's own casing
/// rather than the kebab-case this project uses for formats it defines itself.
const RANGE_UNIT: &str = "DateTime";

/// Reasons a `Content-Range` field value is not an NGSI-LD temporal range.
#[derive(Debug, Error)]
pub enum NgsiLdContentRangeError {
    /// The value did not open with the clause 6.3.10 `DateTime` range unit.
    #[error("content-range must use the '{RANGE_UNIT}' range unit")]
    UnitMissing,
    /// The value carried no `/` separating the range from its size.
    #[error("content-range must separate range and size with '/'")]
    SizeMissing,
    /// The range halves were not separated by a UTC-terminated start instant.
    #[error("content-range must separate range-start and range-end with 'Z-'")]
    RangeMalformed,
    /// One of the two instants was not a clause 4.6.3 `DateTime`, which
    /// clause 6.3.10 makes `range-start` and `range-end`.
    #[error("content-range instant is not an NGSI-LD DateTime: {0}")]
    Instant(#[from] NgsiLdDateTimeError),
    /// The size was neither `*` nor a count.
    #[error("content-range size must be '*' or a count: {0}")]
    Size(#[from] ParseIntError),
}

/// A temporal NGSI-LD content range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdContentRange {
    /// First instant the answered range covers.
    start: DateTime<Utc>,
    /// Last instant the answered range covers.
    end: DateTime<Utc>,
    /// Total number of instances the full range holds, when it is known.
    total: Option<u64>,
}

impl NgsiLdContentRange {
    /// Create a temporal range with an optional total count.
    ///
    /// A backwards `lastN` page legitimately has a range start later than its
    /// range end (ETSI GS CIM 009 clause 6.3.10), so chronological ordering is
    /// not a validity condition for this header.
    #[must_use]
    pub const fn new(start: DateTime<Utc>, end: DateTime<Utc>, total: Option<u64>) -> Self {
        Self { start, end, total }
    }

    /// Start of the temporal range.
    #[must_use]
    pub const fn start(&self) -> DateTime<Utc> {
        self.start
    }
    /// End of the temporal range.
    #[must_use]
    pub const fn end(&self) -> DateTime<Utc> {
        self.end
    }
    /// Total result count, when known.
    #[must_use]
    pub const fn total(&self) -> Option<u64> {
        self.total
    }
}

impl fmt::Display for NgsiLdContentRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The instants keep every sub-second digit they carry: a client resumes
        // a page from `range-end` (clause 6.3.10), and a bound truncated to the
        // second would re-read the instances before it or, as the exclusive
        // `before` bound of a backward page, skip the ones it cut off.
        write!(
            f,
            "{RANGE_UNIT} {}-{}/{}",
            instant_text(self.start),
            instant_text(self.end),
            self.total.map_or_else(|| "*".to_owned(), |n| n.to_string())
        )
    }
}

impl FromStr for NgsiLdContentRange {
    type Err = NgsiLdContentRangeError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let spec = value
            .strip_prefix(RANGE_UNIT)
            .and_then(|rest| rest.strip_prefix(' '))
            .ok_or(NgsiLdContentRangeError::UnitMissing)?;
        let (range, total) = spec.rsplit_once('/').ok_or(NgsiLdContentRangeError::SizeMissing)?;
        let (start, end) = range.split_once("Z-").ok_or(NgsiLdContentRangeError::RangeMalformed)?;
        let start = format!("{start}Z");
        let start = start.parse::<NgsiLdDateTime>()?.get();
        let end = end.parse::<NgsiLdDateTime>()?.get();
        let total = if total == "*" { None } else { Some(total.parse()?) };
        Ok(Self::new(start, end, total))
    }
}

impl headers::Header for NgsiLdContentRange {
    fn name() -> &'static HeaderName {
        &CONTENT_RANGE
    }
    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        values
            .next()
            .ok_or_else(headers::Error::invalid)?
            .to_str()
            .map_err(|_| headers::Error::invalid())?
            .parse()
            .map_err(|_: NgsiLdContentRangeError| headers::Error::invalid())
    }
    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Ok(value) = HeaderValue::from_str(&self.to_string()) {
            values.extend(iter::once(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ngsild_content_range::{NgsiLdContentRange, NgsiLdContentRangeError};
    use andromeda_ngsi_ld_lexicon::time::{date_time::NgsiLdDateTime, rendering::instant_text};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("the fixture is an NGSI-LD DateTime").get()
    }

    #[test]
    fn backwards_last_n_range_accepts_a_later_start_than_end() {
        let range = NgsiLdContentRange::from_str("DateTime 2021-01-01T01:01:00Z-2020-01-01T20:00:00Z/20").expect("valid backwards temporal range");

        assert_eq!(instant_text(range.start()), "2021-01-01T01:01:00Z");
        assert_eq!(instant_text(range.end()), "2020-01-01T20:00:00Z");
        assert_eq!(range.total(), Some(20));
    }

    /// Clause 6.3.10: `"unit" shall be equal to "DateTime"`. The rendered field
    /// value carries the specification's own casing, not a kebab-cased variant.
    #[test]
    fn the_rendered_range_unit_is_the_spec_mandated_datetime_token() {
        let range = NgsiLdContentRange::new(instant("2020-01-01T20:00:00Z"), instant("2021-01-01T01:01:00Z"), None);

        assert_eq!(range.to_string(), "DateTime 2020-01-01T20:00:00Z-2021-01-01T01:01:00Z/*");
    }

    /// A size is rendered verbatim when the total is known.
    #[test]
    fn a_known_total_renders_as_the_range_size() {
        let range = NgsiLdContentRange::new(instant("2020-01-01T20:00:00Z"), instant("2021-01-01T01:01:00Z"), Some(20));

        assert_eq!(range.to_string(), "DateTime 2020-01-01T20:00:00Z-2021-01-01T01:01:00Z/20");
    }

    /// The pre-1.9.1 `date-time` spelling is not the clause 6.3.10 unit and is refused.
    #[test]
    fn the_kebab_cased_range_unit_is_rejected() {
        let error = NgsiLdContentRange::from_str("date-time 2020-01-01T20:00:00Z-2021-01-01T01:01:00Z/*").expect_err("must reject");

        assert!(matches!(error, NgsiLdContentRangeError::UnitMissing), "got {error:?}");
    }

    /// A client resumes a page from its bounds, so an instant keeps the
    /// sub-second digits it carries rather than being truncated to the second.
    #[test]
    fn a_sub_second_bound_is_rendered_without_losing_precision() {
        let range = NgsiLdContentRange::new(instant("2020-01-01T20:00:00.250Z"), instant("2020-01-01T20:00:01.123456Z"), None);

        assert_eq!(range.to_string(), "DateTime 2020-01-01T20:00:00.250Z-2020-01-01T20:00:01.123456Z/*");
        assert_eq!(NgsiLdContentRange::from_str(&range.to_string()).expect("round trip"), range);
    }

    /// Clause 6.3.10 makes both bounds clause 4.6.3 `DateTime` values.
    #[test]
    fn a_bound_clause_4_6_3_refuses_is_refused() {
        for value in [
            "DateTime 2020-01-01T20:00:00Z-2021-01-01T01:01:00.1234567Z/*",
            "DateTime 2020-01-01T20:00:00Z-2021-01-01T01:01Z/*",
        ] {
            let error = NgsiLdContentRange::from_str(value).expect_err("must reject");
            assert!(matches!(error, NgsiLdContentRangeError::Instant(_)), "{value}: got {error:?}");
        }
    }

    /// Rendering and parsing agree, so a range survives a round trip.
    #[test]
    fn a_rendered_range_parses_back_to_the_same_value() {
        let range = NgsiLdContentRange::new(instant("2020-01-01T20:00:00Z"), instant("2021-01-01T01:01:00Z"), Some(7));

        assert_eq!(NgsiLdContentRange::from_str(&range.to_string()).expect("round trip"), range);
    }
}
