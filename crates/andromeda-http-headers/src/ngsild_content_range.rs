// SPDX-FileCopyrightText: 2014-2025 Sean McArthur
// SPDX-License-Identifier: MIT

//! Typed NGSI-LD temporal `Content-Range` response header (ETSI GS CIM 009,
//! clause 6.3.10).
//!
//! This file is derived from the `ContentRange` implementation in
//! <https://github.com/hyperium/headers>, extended for the NGSI-LD `DateTime`
//! range unit rather than byte ranges.
//! This file, including local modifications, is distributed under the MIT License.

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
    #[error("Content-Range must use the '{RANGE_UNIT}' range unit: '{0}'")]
    UnitMissing(String),
    /// The value carried no `/` separating the range from its size.
    #[error("Content-Range must separate range and size with '/': '{0}'")]
    SizeMissing(String),
    /// The range halves were not separated by a UTC-terminated start instant.
    #[error("Content-Range must separate range-start and range-end with 'Z-': '{0}'")]
    RangeMalformed(String),
    /// One of the two instants was not an RFC 3339 timestamp.
    #[error("Content-Range instant is not an RFC 3339 timestamp: {0}")]
    Instant(#[from] chrono::ParseError),
    /// The size was neither `*` nor a count.
    #[error("Content-Range size must be '*' or a count: {0}")]
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
        write!(
            f,
            "{RANGE_UNIT} {}-{}/{}",
            self.start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
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
            .ok_or_else(|| NgsiLdContentRangeError::UnitMissing(value.to_owned()))?;
        let (range, total) = spec.rsplit_once('/').ok_or_else(|| NgsiLdContentRangeError::SizeMissing(value.to_owned()))?;
        let (start, end) = range
            .split_once("Z-")
            .ok_or_else(|| NgsiLdContentRangeError::RangeMalformed(value.to_owned()))?;
        let start = format!("{start}Z");
        let start = DateTime::parse_from_rfc3339(&start)?.with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339(end)?.with_timezone(&Utc);
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
    use chrono::{DateTime, Utc};
    use std::str::FromStr;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .expect("the fixture is an RFC 3339 timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn backwards_last_n_range_accepts_a_later_start_than_end() {
        let range = NgsiLdContentRange::from_str("DateTime 2021-01-01T01:01:00Z-2020-01-01T20:00:00Z/20").expect("valid backwards temporal range");

        assert_eq!(range.start().to_rfc3339(), "2021-01-01T01:01:00+00:00");
        assert_eq!(range.end().to_rfc3339(), "2020-01-01T20:00:00+00:00");
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

        assert!(matches!(error, NgsiLdContentRangeError::UnitMissing(_)), "got {error:?}");
    }

    /// Rendering and parsing agree, so a range survives a round trip.
    #[test]
    fn a_rendered_range_parses_back_to_the_same_value() {
        let range = NgsiLdContentRange::new(instant("2020-01-01T20:00:00Z"), instant("2021-01-01T01:01:00Z"), Some(7));

        assert_eq!(NgsiLdContentRange::from_str(&range.to_string()).expect("round trip"), range);
    }
}
