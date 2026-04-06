//! Typed `NGSILD-Snapshot` request/response header (ETSI GS CIM 009 clause 6.3.22).
//!
//! Carries the snapshot identifier for snapshot-scoped requests and responses.
//! The value must be a non-empty URI string.

use axum::http::{HeaderName, HeaderValue};

static NGSILD_SNAPSHOT: HeaderName = HeaderName::from_static("ngsild-snapshot");

/// Typed `NGSILD-Snapshot` header (clause 6.3.22).
///
/// Carries the snapshot identifier URI that scopes a request to a frozen
/// data snapshot, or indicates that a response is from a snapshot.
///
/// TODO: the inner type should be `urn::Urn` instead of `String` to enforce
/// URN validation at the HTTP boundary. Blocked on `EntityMap.id` and snapshot
/// IDs flowing through the service layer as `String`; once those are migrated
/// to `urn::Urn`, this type should follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdSnapshot(String);

impl NgsiLdSnapshot {
    /// Create a new `NGSILD-Snapshot` header from the given URI value.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if `value` is empty after trimming.
    pub fn new(value: impl Into<String>) -> NgsiLdSnapshot {
        let s = value.into();
        debug_assert!(!s.trim().is_empty(), "NGSILD-Snapshot value must not be empty");
        NgsiLdSnapshot(s)
    }

    /// Consume the header and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
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
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(NgsiLdSnapshot(trimmed.to_string()))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Ok(val) = HeaderValue::from_str(&self.0) {
            values.extend(std::iter::once(val));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdSnapshot, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdSnapshot::decode(&mut iter)
    }

    #[test]
    fn decode_valid_snapshot() {
        let header = decode_from("urn:ngsi-ld:Snapshot:abc").expect("should decode");
        assert_eq!(header.into_inner(), "urn:ngsi-ld:Snapshot:abc");
    }

    #[test]
    fn decode_trims_whitespace() {
        let header = decode_from("  urn:snap:1  ").expect("should decode");
        assert_eq!(header.into_inner(), "urn:snap:1");
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
    fn encode_roundtrip() {
        let header = NgsiLdSnapshot::new("urn:snap:x");
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].to_str().expect("valid str"), "urn:snap:x");

        let mut iter = encoded.iter();
        let decoded = NgsiLdSnapshot::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(header, decoded);
    }
}
