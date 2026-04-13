//! Typed `NGSILD-Tenant` request/response header (ETSI GS CIM 009 clause 6.3.14).
//!
//! Identifies the tenant scope for multi-tenant deployments.
//! The value must be a non-empty string.

use crate::tenancy::types::TenantName;
use axum::http::{HeaderName, HeaderValue};

static NGSILD_TENANT: HeaderName = HeaderName::from_static("ngsild-tenant");

/// Typed `NGSILD-Tenant` header (clause 6.3.14).
///
/// Carries the tenant identifier for multi-tenant deployments. The value
/// must be a non-empty string after trimming whitespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdTenant(TenantName);

impl NgsiLdTenant {
    /// Create a new `NGSILD-Tenant` header from a validated [`TenantName`].
    pub const fn new(value: TenantName) -> NgsiLdTenant {
        NgsiLdTenant(value)
    }

    /// Return the tenant identifier as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Consume the header and return the inner [`TenantName`].
    pub fn into_inner(self) -> TenantName {
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
        let name = TenantName::try_from(trimmed.to_string()).map_err(|_| headers::Error::invalid())?;
        Ok(NgsiLdTenant(name))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.0.as_str()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Tenant header"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
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
        pretty_assertions::assert_eq!(header.as_str(), "my-tenant");
    }

    #[test]
    fn decode_trims_whitespace() {
        let header = decode_from("  tenant  ").expect("should decode");
        pretty_assertions::assert_eq!(header.as_str(), "tenant");
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
        let name = match TenantName::try_from("t1".to_string()) {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected tenant error: {err}"),
        };
        let header = NgsiLdTenant::new(name);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);
        let encoded_str = encoded[0].to_str();
        assert!(encoded_str.is_ok());
        let encoded_str = match encoded_str {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected header value error: {err}"),
        };
        pretty_assertions::assert_eq!(encoded_str, "t1");

        let mut iter = encoded.iter();
        let decoded = NgsiLdTenant::decode(&mut iter);
        assert!(decoded.is_ok());
        let decoded = match decoded {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected roundtrip decode error: {err}"),
        };
        pretty_assertions::assert_eq!(header, decoded);
    }
}
