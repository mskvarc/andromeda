//! The tenant-management HTTP handlers: the collection, the per-tenant
//! introspection resource, and the suspend/resume lifecycle write.

pub mod collection;
pub mod introspection;
pub mod lifecycle;

use crate::error::{InvalidTenantPathName, TenancyHttpError, classify};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_tenancy::error::TenancyError;
use axum::http::Uri;

/// Parse a path segment into a validated tenant name.
///
/// A malformed name in the path is a syntactically invalid request rather than a
/// missing tenant, so it is answered with `InvalidRequest`.
fn parse_tenant_name(raw: String, uri: &Uri) -> Result<TenantName, ProblemResponse> {
    TenantName::try_from(raw)
        .map_err(|error| ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, InvalidTenantPathName(error).to_string()).with_instance(uri.clone()))
}

/// The 404 `NonexistentTenant` a management request for an absent tenant answers
/// (clause 6.3.2), reusing the shared tenancy classification.
fn nonexistent_tenant(name: &TenantName, uri: &Uri) -> ProblemResponse {
    classify(&TenancyHttpError::Tenancy(TenancyError::UnknownTenant { name: name.clone() })).with_instance(uri.clone()) // clone: the error owns the name
}
