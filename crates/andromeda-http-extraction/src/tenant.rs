//! Tenant header extractor for NGSI-LD multi-tenancy.
//!
//! Extracts the optional `NGSILD-Tenant` header from the request, producing
//! a [`TenantHeader`] that carries the tenant name (or `None` for the default tenant).

use crate::addressed_uri::addressed_uri;
use andromeda_http_headers::ngsild_tenant::NgsiLdTenant;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::Header;
use std::future::{Future, ready};

/// Axum extractor for the `NGSILD-Tenant` request header.
///
/// Returns `Some(TenantName)` when the header is present and valid, `None`
/// when absent, or a `BadRequestData` rejection naming why a present header
/// names no tenant: it is empty, it is not visible ASCII, or it is not a
/// tenant name.
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
    parts
        .headers
        .get(NgsiLdTenant::name())
        .map(NgsiLdTenant::try_from)
        .transpose()
        .map(|tenant| TenantHeader(tenant.map(NgsiLdTenant::into_inner)))
        .map_err(|error| ExtractorRejection::bad_request_data(error.to_string(), addressed_uri(parts)))
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
    use crate::tenant::TenantHeader;
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use axum::{
        extract::FromRequestParts,
        http::{HeaderValue, Request},
    };

    async fn extract(value: HeaderValue) -> Result<TenantHeader, ExtractorRejection> {
        let req = Request::builder().uri("/test").header("NGSILD-Tenant", value).body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        TenantHeader::from_request_parts(&mut parts, &()).await
    }

    async fn refusal_detail(value: &'static str) -> String {
        match extract(HeaderValue::from_static(value)).await {
            Err(ExtractorRejection::BadRequestData { detail, .. }) => detail,
            other => panic!("{value:?} should be refused as BadRequestData, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn valid_tenant_header_extracts_name() {
        let result = extract(HeaderValue::from_static("my-tenant")).await.expect("should succeed");
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
    async fn an_empty_header_is_refused_as_empty() {
        pretty_assertions::assert_eq!(refusal_detail("").await, "the NGSILD-Tenant header is empty");
    }

    #[tokio::test]
    async fn a_whitespace_only_header_is_refused_as_empty() {
        pretty_assertions::assert_eq!(refusal_detail("   ").await, "the NGSILD-Tenant header is empty");
    }

    #[tokio::test]
    async fn a_name_with_a_leading_digit_is_refused_as_not_a_tenant_name() {
        pretty_assertions::assert_eq!(
            refusal_detail("1abc").await,
            "the NGSILD-Tenant header is not a valid tenant name: tenant name must match [A-Za-z_][A-Za-z0-9_-]*"
        );
    }

    #[tokio::test]
    async fn a_name_with_a_space_is_refused_as_not_a_tenant_name() {
        pretty_assertions::assert_eq!(
            refusal_detail("bad tenant").await,
            "the NGSILD-Tenant header is not a valid tenant name: tenant name must match [A-Za-z_][A-Za-z0-9_-]*"
        );
    }

    #[tokio::test]
    async fn a_header_outside_visible_ascii_is_refused_as_not_text() {
        let value = HeaderValue::from_bytes("café".as_bytes()).expect("obs-text is a valid header value");
        match extract(value).await {
            Err(ExtractorRejection::BadRequestData { detail, .. }) => {
                pretty_assertions::assert_eq!(detail, "the NGSILD-Tenant header holds a character outside visible ASCII");
            }
            other => panic!("a non-ASCII tenant should be refused as BadRequestData, got {other:?}"),
        }
    }
}
