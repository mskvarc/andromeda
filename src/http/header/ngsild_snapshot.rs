//! Typed `NGSILD-Snapshot` request/response header (ETSI GS CIM 009 clause 6.3.22).
//!
//! Carries the snapshot identifier for snapshot-scoped requests and responses.
//! The value must be a valid snapshot URN (`urn:ngsi-ld:Snapshot:<id>`).

use andromeda_domain::ngsi_ld_urn::SnapshotId;
use axum::http::{HeaderName, HeaderValue};

static NGSILD_SNAPSHOT: HeaderName = HeaderName::from_static("ngsild-snapshot");

/// Typed `NGSILD-Snapshot` header (clause 6.3.22).
///
/// Carries the snapshot identifier URN that scopes a request to a frozen
/// data snapshot, or indicates that a response is from a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdSnapshot(SnapshotId);

impl NgsiLdSnapshot {
    /// Create a new `NGSILD-Snapshot` header from a validated URN.
    pub const fn new(value: SnapshotId) -> NgsiLdSnapshot {
        NgsiLdSnapshot(value)
    }

    /// Consume the header and return the inner URN.
    pub fn into_inner(self) -> SnapshotId {
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
        let s = match value.to_str() {
            Ok(value) => value,
            Err(_) => return Err(headers::Error::invalid()),
        };
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        let snapshot_id = match trimmed.parse::<SnapshotId>() {
            Ok(value) => value,
            Err(_) => return Err(headers::Error::invalid()),
        };
        Ok(NgsiLdSnapshot(snapshot_id))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.0.as_str()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Snapshot header"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use andromeda_domain::ngsi_ld_urn::SnapshotId;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdSnapshot, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdSnapshot::decode(&mut iter)
    }

    #[test]
    fn decode_valid_snapshot_urn() {
        let header = decode_from("urn:ngsi-ld:Snapshot:abc").expect("should decode");
        pretty_assertions::assert_eq!(header.into_inner().as_str(), "urn:ngsi-ld:Snapshot:abc");
    }

    #[test]
    fn decode_trims_whitespace() {
        let header = decode_from("  urn:ngsi-ld:Snapshot:1  ").expect("should decode");
        pretty_assertions::assert_eq!(header.into_inner().as_str(), "urn:ngsi-ld:Snapshot:1");
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
    fn decode_rejects_plain_url() {
        assert!(decode_from("https://example.com/snapshot").is_err());
    }

    #[test]
    fn encode_roundtrip() {
        let snapshot_id = "urn:ngsi-ld:Snapshot:x".parse::<SnapshotId>().expect("valid snapshot id");
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
