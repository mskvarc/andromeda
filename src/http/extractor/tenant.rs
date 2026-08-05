//! Tenant header extractor for NGSI-LD multi-tenancy.
//!
//! Extracts the optional `NGSILD-Tenant` header from the request, producing
//! a [`TenantHeader`] that carries the tenant name (or `None` for the default tenant).

use crate::http::extractor::error::ExtractorRejection;
use andromeda_ngsi_ld_http_headers::ngsild_tenant::NgsiLdTenant;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;

/// Axum extractor for the `NGSILD-Tenant` request header.
///
/// Returns `Some(TenantName)` when the header is present and valid, `None`
/// when absent, or a `BadRequestData` rejection when the header is present
/// but invalid (e.g. empty).
#[derive(Clone, Debug, Default)]
pub struct TenantHeader(pub Option<TenantName>);

impl TenantHeader {
    /// Consume the extractor and return the optional tenant name.
    pub fn into_inner(self) -> Option<TenantName> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::FromRequestParts, http::Request};

    #[tokio::test]
    async fn valid_tenant_header_extracts_name() {
        let req = Request::builder()
            .uri("/test")
            .header("NGSILD-Tenant", "my-tenant")
            .body(())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = TenantHeader::from_request_parts(&mut parts, &()).await.expect("should succeed");
        let name = result.into_inner().expect("should have value");
        pretty_assertions::assert_eq!(name.as_str(), "my-tenant");
    }

    #[tokio::test]
    async fn no_header_returns_none() {
        let req = Request::builder().uri("/test").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = TenantHeader::from_request_parts(&mut parts, &()).await.expect("should succeed");
        assert!(result.into_inner().is_none());
    }

    #[tokio::test]
    async fn empty_header_produces_rejection() {
        let req = Request::builder().uri("/test").header("NGSILD-Tenant", "").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = TenantHeader::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
