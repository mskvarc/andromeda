//! Validated tenant header value carried by a test client.

use crate::transport_error::TenantNameError;
use http::HeaderValue;

/// A validated `NGSILD-Tenant` header value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TenantName {
    header: HeaderValue,
}

impl TenantName {
    /// Validate a tenant name as an HTTP header value.
    pub fn parse(value: impl AsRef<str>) -> Result<TenantName, TenantNameError> {
        let header = HeaderValue::from_str(value.as_ref()).map_err(TenantNameError::InvalidHeaderValue)?;
        Ok(TenantName { header })
    }

    /// Borrow the encoded tenant header value.
    pub const fn header_value(&self) -> &HeaderValue {
        &self.header
    }
}
