//! Typed `NGSILD-EntityMap` request/response header (ETSI GS CIM 009 clause 6.4.3.2).
//!
//! Carries the entity map URI for paginated distributed queries.
//! The value must be a valid URN (RFC 8141).

use http::{HeaderName, HeaderValue};
use urn_rs::Urn;

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_ENTITYMAP: HeaderName = HeaderName::from_static("ngsild-entitymap");

/// Typed `NGSILD-EntityMap` header (clause 6.4.3.2, Table 6.4.3.2-2).
///
/// Carries the URN of an entity map resource created during a distributed
/// query, used for stable pagination across context sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdEntityMap(Urn);

impl NgsiLdEntityMap {
    /// Create a new `NGSILD-EntityMap` header from a validated URN.
    pub const fn new(value: Urn) -> NgsiLdEntityMap {
        NgsiLdEntityMap(value)
    }

    /// Return the entity map URN as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Consume the header and return the inner URN.
    pub fn into_inner(self) -> Urn {
        self.0
    }
}

impl headers::Header for NgsiLdEntityMap {
    fn name() -> &'static HeaderName {
        &NGSILD_ENTITYMAP
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdEntityMap, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        let urn: Urn = trimmed.parse().map_err(|_| headers::Error::invalid())?;
        Ok(NgsiLdEntityMap(urn))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.0.as_str()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-EntityMap header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdEntityMap, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdEntityMap::decode(&mut iter)
    }

    #[test]
    fn decode_valid_entity_map_urn() {
        let header = decode_from("urn:ngsi-ld:EntityMap:abc123").expect("should decode");
        pretty_assertions::assert_eq!(header.as_str(), "urn:ngsi-ld:EntityMap:abc123");
    }

    #[test]
    fn decode_trims_whitespace() {
        let header = decode_from("  urn:ngsi-ld:EntityMap:x  ").expect("should decode");
        pretty_assertions::assert_eq!(header.as_str(), "urn:ngsi-ld:EntityMap:x");
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
        assert!(decode_from("https://example.com/entitymap").is_err());
    }

    #[test]
    fn encode_roundtrip() {
        let urn: Urn = "urn:ngsi-ld:EntityMap:test".parse().expect("valid URN");
        let header = NgsiLdEntityMap::new(urn);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "urn:ngsi-ld:EntityMap:test");

        let mut iter = encoded.iter();
        let decoded = NgsiLdEntityMap::decode(&mut iter).expect("roundtrip decode");
        pretty_assertions::assert_eq!(header, decoded);
    }
}
