//! Tenant header extractor for NGSI-LD multi-tenancy.
//!
//! Extracts the optional `NGSILD-Tenant` header from the request, producing
//! a [`TenantHeader`] that carries the tenant name (or `None` for the default tenant).

use crate::http::{extractor::error::ExtractorRejection, header::NgsiLdTenant};
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;

/// Axum extractor for the `NGSILD-Tenant` request header.
///
/// Returns `Some(name)` when the header is present and valid, `None` when
/// absent, or a `BadRequestData` rejection when the header is present but
/// invalid (e.g. empty).
#[derive(Clone, Debug, Default)]
pub struct TenantHeader(pub Option<String>);

impl TenantHeader {
    /// Consume the extractor and return the optional tenant name.
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
