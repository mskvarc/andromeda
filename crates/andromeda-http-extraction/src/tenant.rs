//! Tenant header extractor for NGSI-LD multi-tenancy.
//!
//! Extracts the optional `NGSILD-Tenant` header from the request, producing
//! a [`TenantHeader`] that carries the tenant name (or `None` for the default tenant).

use crate::addressed_uri::addressed_uri;
use andromeda_http_headers::ngsild_tenant::NgsiLdTenant;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use std::future::{Future, ready};

/// Axum extractor for the `NGSILD-Tenant` request header.
///
/// Returns `Some(TenantName)` when the header is present and valid, `None`
/// when absent, or a `BadRequestData` rejection when the header is present
/// but invalid (e.g. empty).
#[derive(Clone, Debug, Default)]
pub struct TenantHeader(pub Option<TenantName>);

impl TenantHeader {
    /// Consume the extractor and return the optional tenant name.
    #[must_use]
    pub fn into_inner(self) -> Option<TenantName> {
        self.0
    }
}

/// Decode the `NGSILD-Tenant` header into the tenant the request addresses,
/// rejecting a header that is present but not a tenant name.
fn resolve_tenant_header(parts: &Parts) -> Result<TenantHeader, ExtractorRejection> {
    match parts.headers.typed_try_get::<NgsiLdTenant>() {
        Ok(Some(tenant)) => Ok(TenantHeader(Some(tenant.into_inner()))),
        Ok(None) => Ok(TenantHeader(None)),
        Err(_) => Err(ExtractorRejection::bad_request_data(
            "NGSILD-Tenant header must not be empty",
            addressed_uri(parts),
        )),
    }
}

impl<S> FromRequestParts<S> for TenantHeader
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_tenant_header(parts))
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
        let (mut parts, ()) = req.into_parts();
        let result = TenantHeader::from_request_parts(&mut parts, &()).await.expect("should succeed");
        let name = result.into_inner().expect("should have value");
        pretty_assertions::assert_eq!(name.as_str(), "my-tenant");
    }

    #[tokio::test]
    async fn no_header_returns_none() {
        let req = Request::builder().uri("/test").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = TenantHeader::from_request_parts(&mut parts, &()).await.expect("should succeed");
        assert!(result.into_inner().is_none());
    }

    #[tokio::test]
    async fn empty_header_produces_rejection() {
        let req = Request::builder().uri("/test").header("NGSILD-Tenant", "").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = TenantHeader::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
