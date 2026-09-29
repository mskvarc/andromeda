//! Typed `NGSILD-Snapshot` request/response header (ETSI GS CIM 009 clause 6.3.22).
//!
//! Carries the snapshot identifier for snapshot-scoped requests and responses.
//! The value must be a valid URI.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use http::{HeaderName, HeaderValue};
use std::iter;

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_SNAPSHOT: HeaderName = HeaderName::from_static("ngsild-snapshot");

/// Typed `NGSILD-Snapshot` header (clause 6.3.22).
///
/// Carries the snapshot identifier URI that scopes a request to a frozen
/// data snapshot, or indicates that a response is from a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdSnapshot(Identifier);

impl NgsiLdSnapshot {
    /// Create a new `NGSILD-Snapshot` header from a validated URI.
    #[must_use]
    pub const fn new(value: Identifier) -> NgsiLdSnapshot {
        NgsiLdSnapshot(value)
    }

    /// Consume the header and return the inner URI.
    #[must_use]
    pub fn into_inner(self) -> Identifier {
        self.0
    }

    /// Return the Snapshot identifier as its wire representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl headers::Header for NgsiLdSnapshot {
    fn name() -> &'static HeaderName {
        &NGSILD_SNAPSHOT
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdSnapshot, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let Ok(s) = value.to_str() else {
            return Err(headers::Error::invalid());
        };
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed != s || trimmed.chars().any(char::is_whitespace) {
            return Err(headers::Error::invalid());
        }
        let Ok(snapshot_id) = trimmed.parse::<Identifier>() else {
            return Err(headers::Error::invalid());
        };
        Ok(NgsiLdSnapshot(snapshot_id))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.0.as_str()) {
            Ok(val) => values.extend(iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Snapshot header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdSnapshot, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = iter::once(&val);
        NgsiLdSnapshot::decode(&mut iter)
    }

    #[test]
    fn decode_valid_snapshot_urn() {
        let header = decode_from("urn:ngsi-ld:Snapshot:abc").expect("should decode");
        pretty_assertions::assert_eq!(header.into_inner().as_str(), "urn:ngsi-ld:Snapshot:abc");
    }

    #[test]
    fn decode_rejects_surrounding_whitespace() {
        assert!(decode_from("  urn:ngsi-ld:Snapshot:1  ").is_err());
    }

    #[test]
    fn decode_rejects_empty() {
        assert!(decode_from("").is_err());
    }

    #[test]
    fn decode_rejects_whitespace_only() {
        assert!(decode_from("   ").is_err());
    }

    #[test]
    fn decode_rejects_non_urn() {
        assert!(decode_from("not-a-urn").is_err());
    }

    #[test]
    fn decode_rejects_internal_uri_whitespace() {
        assert!(decode_from("urn:ngsi-ld:Snapshot:header matrix").is_err());
    }

    #[test]
    fn decode_accepts_https_snapshot_uri() {
        let header = decode_from("https://example.com/snapshot").expect("valid snapshot URI");
        pretty_assertions::assert_eq!(header.into_inner().as_str(), "https://example.com/snapshot");
    }

    #[test]
    fn encode_roundtrip() {
        let snapshot_id = "urn:ngsi-ld:Snapshot:x".parse::<Identifier>().expect("valid snapshot id");
        let header = NgsiLdSnapshot::new(snapshot_id);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "urn:ngsi-ld:Snapshot:x");

        let mut iter = encoded.iter();
        let decoded = NgsiLdSnapshot::decode(&mut iter).expect("roundtrip decode");
        pretty_assertions::assert_eq!(header, decoded);
    }
}
