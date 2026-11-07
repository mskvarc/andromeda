//! Typed `NGSILD-Tenant` request/response header (ETSI GS CIM 009 clause 6.3.14).
//!
//! Identifies the tenant scope for multi-tenant deployments. The value, once
//! its surrounding whitespace is trimmed, must be a [`TenantName`].

use andromeda_ngsi_ld_lexicon::tenant::{InvalidTenantName, TenantName};
use http::{HeaderName, HeaderValue};
use std::iter;
use thiserror::Error;

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_TENANT: HeaderName = HeaderName::from_static("ngsild-tenant");

/// Typed `NGSILD-Tenant` header (clause 6.3.14).
///
/// Carries the tenant identifier for multi-tenant deployments. The value
/// must be a [`TenantName`] after trimming whitespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdTenant(TenantName);

/// Why an `NGSILD-Tenant` header value names no tenant.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NgsiLdTenantError {
    /// The value is empty or holds only whitespace.
    #[error("the NGSILD-Tenant header is empty")]
    Empty,
    /// The value holds a byte outside visible ASCII, which no tenant name contains.
    #[error("the NGSILD-Tenant header holds a character outside visible ASCII")]
    NotText,
    /// The value is text but breaks the tenant name grammar or its length bound.
    #[error("the NGSILD-Tenant header is not a valid tenant name: {0}")]
    InvalidName(#[source] InvalidTenantName),
}

impl NgsiLdTenant {
    /// Create a new `NGSILD-Tenant` header from a validated [`TenantName`].
    #[must_use]
    pub const fn new(value: TenantName) -> NgsiLdTenant {
        NgsiLdTenant(value)
    }

    /// Return the tenant identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Consume the header and return the inner [`TenantName`].
    #[must_use]
    pub fn into_inner(self) -> TenantName {
        self.0
    }
}

impl TryFrom<&HeaderValue> for NgsiLdTenant {
    type Error = NgsiLdTenantError;

    /// Read one `NGSILD-Tenant` field value.
    ///
    /// # Errors
    ///
    /// Returns [`NgsiLdTenantError::NotText`] for a value holding a byte outside
    /// visible ASCII, [`NgsiLdTenantError::Empty`] for a value that is empty once
    /// trimmed, and [`NgsiLdTenantError::InvalidName`] for one that is not a
    /// [`TenantName`].
    fn try_from(value: &HeaderValue) -> Result<NgsiLdTenant, NgsiLdTenantError> {
        let trimmed = value.to_str().map_err(|_| NgsiLdTenantError::NotText)?.trim();
        if trimmed.is_empty() {
            return Err(NgsiLdTenantError::Empty);
        }
        trimmed.parse().map(NgsiLdTenant).map_err(NgsiLdTenantError::InvalidName)
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
        NgsiLdTenant::try_from(value).map_err(|_| headers::Error::invalid())
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.0.as_str()) {
            Ok(val) => values.extend(iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Tenant header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ngsild_tenant::{NgsiLdTenant, NgsiLdTenantError};
    use andromeda_ngsi_ld_lexicon::tenant::{InvalidTenantName, TENANT_NAME_BYTE_LIMIT, TenantName};
    use headers::Header;
    use http::HeaderValue;
    use std::iter;

    fn read(raw: &str) -> Result<NgsiLdTenant, NgsiLdTenantError> {
        let value = HeaderValue::from_str(raw).expect("valid header value");
        NgsiLdTenant::try_from(&value)
    }

    #[test]
    fn a_tenant_name_is_read() {
        pretty_assertions::assert_eq!(read("my-tenant").expect("should read").as_str(), "my-tenant");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        pretty_assertions::assert_eq!(read("  tenant  ").expect("should read").as_str(), "tenant");
    }

    #[test]
    fn an_empty_value_is_empty() {
        pretty_assertions::assert_eq!(read(""), Err(NgsiLdTenantError::Empty));
    }

    #[test]
    fn a_whitespace_only_value_is_empty() {
        pretty_assertions::assert_eq!(read("   "), Err(NgsiLdTenantError::Empty));
    }

    #[test]
    fn a_value_breaking_the_grammar_is_an_invalid_name() {
        pretty_assertions::assert_eq!(read("1abc"), Err(NgsiLdTenantError::InvalidName(InvalidTenantName::InvalidSyntax)));
        pretty_assertions::assert_eq!(read("bad tenant"), Err(NgsiLdTenantError::InvalidName(InvalidTenantName::InvalidSyntax)));
    }

    #[test]
    fn a_value_at_the_tenant_name_limit_is_read() {
        let at_limit = "a".repeat(TENANT_NAME_BYTE_LIMIT);
        pretty_assertions::assert_eq!(read(&at_limit).expect("should read").as_str(), at_limit);
    }

    #[test]
    fn an_over_long_value_is_an_invalid_name_stating_the_limit() {
        let refused = read(&"a".repeat(TENANT_NAME_BYTE_LIMIT + 1));
        pretty_assertions::assert_eq!(
            refused,
            Err(NgsiLdTenantError::InvalidName(InvalidTenantName::TooLong {
                length: TENANT_NAME_BYTE_LIMIT + 1
            }))
        );
        pretty_assertions::assert_eq!(
            refused.err().map(|error| error.to_string()),
            Some("the NGSILD-Tenant header is not a valid tenant name: a tenant name of 1006 bytes exceeds the 1005 bytes a tenant name may occupy".to_owned())
        );
    }

    #[test]
    fn a_value_outside_visible_ascii_is_not_text() {
        let value = HeaderValue::from_bytes("café".as_bytes()).expect("obs-text is a valid header value");
        pretty_assertions::assert_eq!(NgsiLdTenant::try_from(&value), Err(NgsiLdTenantError::NotText));
    }

    #[test]
    fn each_refusal_names_its_cause() {
        pretty_assertions::assert_eq!(NgsiLdTenantError::Empty.to_string(), "the NGSILD-Tenant header is empty");
        pretty_assertions::assert_eq!(
            NgsiLdTenantError::NotText.to_string(),
            "the NGSILD-Tenant header holds a character outside visible ASCII"
        );
        pretty_assertions::assert_eq!(
            NgsiLdTenantError::InvalidName(InvalidTenantName::InvalidSyntax).to_string(),
            "the NGSILD-Tenant header is not a valid tenant name: tenant name must match [A-Za-z_][A-Za-z0-9_-]*"
        );
    }

    #[test]
    fn the_typed_header_decoder_refuses_what_the_reader_refuses() {
        let value = HeaderValue::from_static("1abc");
        assert!(NgsiLdTenant::decode(&mut iter::once(&value)).is_err());
    }

    #[test]
    fn an_encoded_tenant_decodes_to_itself() {
        let name: TenantName = "t1".parse().expect("valid tenant name");
        let header = NgsiLdTenant::new(name);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded, vec![HeaderValue::from_static("t1")]);
        pretty_assertions::assert_eq!(NgsiLdTenant::decode(&mut encoded.iter()).expect("should decode"), header);
    }
}
