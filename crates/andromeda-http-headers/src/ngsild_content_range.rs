//! Typed NGSI-LD temporal `Content-Range` response header (ETSI GS CIM 009,
//! clause 6.3.10).
//!
//! The type is based on hyperium's `headers::ContentRange` implementation
//! (MIT licensed, <https://github.com/hyperium/headers>), extended for the
//! NGSI-LD `date-time` range unit rather than byte ranges.

use chrono::{DateTime, Utc};
use http::{HeaderName, HeaderValue};
use std::{fmt, str::FromStr};

static CONTENT_RANGE: HeaderName = HeaderName::from_static("content-range");

/// A temporal NGSI-LD content range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdContentRange {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    total: Option<u64>,
}

impl NgsiLdContentRange {
    /// Create a temporal range with an optional total count.
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>, total: Option<u64>) -> Option<Self> {
        (start <= end).then_some(Self { start, end, total })
    }

    /// Start of the temporal range.
    pub const fn start(&self) -> DateTime<Utc> {
        self.start
    }
    /// End of the temporal range.
    pub const fn end(&self) -> DateTime<Utc> {
        self.end
    }
    /// Total result count, when known.
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
        let end = format!("Z{end}");
        let start = DateTime::parse_from_rfc3339(start).map_err(|_| ())?.with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339(&end).map_err(|_| ())?.with_timezone(&Utc);
        let total = if total == "*" { None } else { Some(total.parse().map_err(|_| ())?) };
        Self::new(start, end, total).ok_or(())
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
            .map_err(|_| headers::Error::invalid())
    }
    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Ok(value) = HeaderValue::from_str(&self.to_string()) {
            values.extend(std::iter::once(value));
        }
    }
}
