//! Typed `NGSILD-EntityMap` request/response header (ETSI GS CIM 009 clause 6.4.3.2).
//!
//! Carries the entity map URI for paginated distributed queries.
//! The value must be a non-empty URI string.

use axum::http::{HeaderName, HeaderValue};

static NGSILD_ENTITYMAP: HeaderName = HeaderName::from_static("ngsild-entitymap");

/// Typed `NGSILD-EntityMap` header (clause 6.4.3.2, Table 6.4.3.2-2).
///
/// Carries the URI of an entity map resource created during a distributed
/// query, used for stable pagination across context sources.
///
/// TODO: the inner type should be `urn::Urn` instead of `String` to enforce
/// URN validation at the HTTP boundary. Blocked on `EntityMap.id` and the
/// entity map service layer using `String`; once those are migrated to
/// `urn::Urn`, this type should follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdEntityMap(String);

impl NgsiLdEntityMap {
    /// Create a new `NGSILD-EntityMap` header from the given URI value.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if `value` is empty after trimming.
    pub fn new(value: impl Into<String>) -> NgsiLdEntityMap {
        let s = value.into();
        debug_assert!(!s.trim().is_empty(), "NGSILD-EntityMap value must not be empty");
        NgsiLdEntityMap(s)
    }

    /// Return the entity map URI as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the header and return the inner string.
    pub fn into_inner(self) -> String {
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
        Ok(NgsiLdEntityMap(trimmed.to_string()))
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

    fn decode_from(raw: &str) -> Result<NgsiLdEntityMap, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdEntityMap::decode(&mut iter)
    }

    #[test]
    fn decode_valid_entity_map() {
        let header = decode_from("urn:ngsi-ld:EntityMap:abc123").expect("should decode");
        assert_eq!(header.as_str(), "urn:ngsi-ld:EntityMap:abc123");
    }

    #[test]
    fn decode_trims_whitespace() {
        let header = decode_from("  urn:ngsi-ld:EntityMap:x  ").expect("should decode");
        assert_eq!(header.as_str(), "urn:ngsi-ld:EntityMap:x");
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
        let header = NgsiLdEntityMap::new("urn:ngsi-ld:EntityMap:test");
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].to_str().expect("valid str"), "urn:ngsi-ld:EntityMap:test");

        let mut iter = encoded.iter();
        let decoded = NgsiLdEntityMap::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(header, decoded);
    }
}
