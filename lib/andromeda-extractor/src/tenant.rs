use axum::{extract::FromRequestParts, http::request::Parts};

use crate::error::ExtractorRejection;

pub const NGSILD_TENANT_HEADER: &str = "NGSILD-Tenant";

#[derive(Clone, Debug, Default)]
pub struct TenantHeader(pub Option<String>);

impl TenantHeader {
    pub fn into_inner(self) -> Option<String> {
        self.0
    }
}

impl<S> FromRequestParts<S> for TenantHeader
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();
        let Some(raw) = parts.headers.get(NGSILD_TENANT_HEADER) else {
            return Ok(Self(None));
        };

        let value = raw
            .to_str()
            .map_err(|_| ExtractorRejection::bad_request_data("Invalid NGSILD-Tenant header", uri.clone()))?
            .trim();

        if value.is_empty() {
            return Err(ExtractorRejection::bad_request_data("NGSILD-Tenant header must not be empty", uri));
        }

        Ok(Self(Some(value.to_string())))
    }
}
