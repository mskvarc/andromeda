// SPDX-FileCopyrightText: 2014-2025 Sean McArthur
// SPDX-License-Identifier: MIT

//! Typed NGSI-LD temporal `Content-Range` response header (ETSI GS CIM 009,
//! clause 6.3.10).
//!
//! This file is derived from the `ContentRange` implementation in
//! <https://github.com/hyperium/headers>, extended for the NGSI-LD `date-time`
//! range unit rather than byte ranges.
//! This file, including local modifications, is distributed under the MIT License.

use chrono::{DateTime, Utc};
use http::{HeaderName, HeaderValue};
use std::{fmt, iter, str::FromStr};

/// The lowercase `Content-Range` field name this header is transmitted under.
static CONTENT_RANGE: HeaderName = HeaderName::from_static("content-range");

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
            "date-time {}-{}/{}",
            self.start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.total.map_or_else(|| "*".to_owned(), |n| n.to_string())
        )
    }
}

impl FromStr for NgsiLdContentRange {
    type Err = ();
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let spec = value.strip_prefix("date-time ").ok_or(())?;
        let (range, total) = spec.rsplit_once('/').ok_or(())?;
        let (start, end) = range.split_once("Z-").ok_or(())?;
        let start = format!("{start}Z");
        let start = DateTime::parse_from_rfc3339(&start).map_err(|_| ())?.with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339(end).map_err(|_| ())?.with_timezone(&Utc);
        let total = if total == "*" { None } else { Some(total.parse().map_err(|_| ())?) };
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
            .map_err(|()| headers::Error::invalid())
    }
    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Ok(value) = HeaderValue::from_str(&self.to_string()) {
            values.extend(iter::once(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NgsiLdContentRange;
    use std::str::FromStr;

    #[test]
    fn backwards_last_n_range_accepts_a_later_start_than_end() {
        let range = NgsiLdContentRange::from_str("date-time 2021-01-01T01:01:00Z-2020-01-01T20:00:00Z/20").expect("valid backwards temporal range");

        assert_eq!(range.start().to_rfc3339(), "2021-01-01T01:01:00+00:00");
        assert_eq!(range.end().to_rfc3339(), "2020-01-01T20:00:00+00:00");
        assert_eq!(range.total(), Some(20));
    }
}
