use crate::{api::extractor::error::ExtractorRejection, http::header::NgsiLdTenant};
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;

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

        match parts.headers.typed_try_get::<NgsiLdTenant>() {
            Ok(Some(tenant)) => Ok(Self(Some(tenant.into_inner()))),
            Ok(None) => Ok(Self(None)),
            Err(_) => Err(ExtractorRejection::bad_request_data("NGSILD-Tenant header must not be empty", uri)),
        }
    }
}
