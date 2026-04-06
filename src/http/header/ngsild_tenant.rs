//! Typed `NGSILD-Tenant` request/response header (ETSI GS CIM 009 clause 6.3.14).
//!
//! Identifies the tenant scope for multi-tenant deployments.
//! The value must be a non-empty string.

use axum::http::{HeaderName, HeaderValue};

static NGSILD_TENANT: HeaderName = HeaderName::from_static("ngsild-tenant");

/// Typed `NGSILD-Tenant` header (clause 6.3.14).
///
/// Carries the tenant identifier for multi-tenant deployments. The value
/// must be a non-empty string after trimming whitespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdTenant(String);

impl NgsiLdTenant {
    /// Create a new `NGSILD-Tenant` header from the given value.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if `value` is empty after trimming.
    pub fn new(value: impl Into<String>) -> NgsiLdTenant {
        let s = value.into();
        debug_assert!(!s.trim().is_empty(), "NGSILD-Tenant value must not be empty");
        NgsiLdTenant(s)
    }

    /// Return the tenant identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the header and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl headers::Header for NgsiLdTenant {
    fn name() -> &'static HeaderName {
        &NGSILD_TENANT
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdTenant, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(NgsiLdTenant(trimmed.to_string()))
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

    fn decode_from(raw: &str) -> Result<NgsiLdTenant, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdTenant::decode(&mut iter)
    }

    #[test]
    fn decode_valid_tenant() {
        let header = decode_from("my-tenant").expect("should decode");
        assert_eq!(header.as_str(), "my-tenant");
    }

    #[test]
    fn decode_trims_whitespace() {
        let header = decode_from("  tenant  ").expect("should decode");
        assert_eq!(header.as_str(), "tenant");
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
        let header = NgsiLdTenant::new("t1");
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].to_str().expect("valid str"), "t1");

        let mut iter = encoded.iter();
        let decoded = NgsiLdTenant::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(header, decoded);
    }
}
