//! Typed `NGSILD-Results-Count` response header (ETSI GS CIM 009 clause 6.3.13).
//!
//! Contains the total count of matching entities for paginated queries.
//! The value is a non-negative integer.

use axum::http::{HeaderName, HeaderValue};

static NGSILD_RESULTS_COUNT: HeaderName = HeaderName::from_static("ngsild-results-count");

/// Typed `NGSILD-Results-Count` response header (clause 6.3.13).
///
/// Carries the total number of matching entities in a paginated query response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NgsiLdResultsCount(pub(crate) usize);

impl NgsiLdResultsCount {
    /// Create a new `NGSILD-Results-Count` header with the given count.
    pub const fn new(count: usize) -> NgsiLdResultsCount {
        NgsiLdResultsCount(count)
    }
}

impl headers::Header for NgsiLdResultsCount {
    fn name() -> &'static HeaderName {
        &NGSILD_RESULTS_COUNT
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdResultsCount, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let count: usize = s.trim().parse().map_err(|_| headers::Error::invalid())?;
        Ok(NgsiLdResultsCount(count))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Ok(val) = HeaderValue::from_str(&self.0.to_string()) {
            values.extend(std::iter::once(val));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdResultsCount, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdResultsCount::decode(&mut iter)
    }

    #[test]
    fn decode_valid_integer() {
        let header = decode_from("42").expect("should decode");
        assert_eq!(header.0, 42);
    }

    #[test]
    fn decode_rejects_non_numeric() {
        assert!(decode_from("abc").is_err());
    }

    #[test]
    fn decode_rejects_negative() {
        assert!(decode_from("-1").is_err());
    }

    #[test]
    fn encode_roundtrip() {
        let header = NgsiLdResultsCount::new(100);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].to_str().expect("valid str"), "100");

        let mut iter = encoded.iter();
        let decoded = NgsiLdResultsCount::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(header, decoded);
    }

    #[test]
    fn decode_zero() {
        let header = decode_from("0").expect("should decode zero");
        assert_eq!(header.0, 0);
    }
}
