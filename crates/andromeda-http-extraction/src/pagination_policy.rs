//! The broker's pagination policy, as a request reads it.
//!
//! The policy is decided once at startup from `[pagination]` and
//! `[extensions]`, and the API assembly installs it as a request extension of
//! every NGSI-LD request. A paged collection reads it through
//! [`RequestPagination`], so no handler can page by a default of its own.

use crate::addressed_uri::addressed_uri;
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    extraction_rejection::ExtractorRejection,
    internal_failure_summary::InternalFailureSummary,
};
use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicy;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};
use thiserror::Error;

/// The pagination policy the API assembly installed for this request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestPagination(pub PaginationPolicy);

/// The request reached a paged collection without the policy installed: a
/// wiring fault of the broker, never a client's.
#[derive(Debug, Error)]
#[error("the request carries no pagination policy; the API assembly installs one on every NGSI-LD request")]
struct PaginationPolicyNotInstalled;

/// The installed policy, or the internal error its absence is.
fn installed_policy(parts: &Parts) -> Result<RequestPagination, ExtractorRejection> {
    parts.extensions.get::<PaginationPolicy>().copied().map(RequestPagination).ok_or_else(|| {
        let report = ClassifiedReport::internal(&PaginationPolicyNotInstalled, InternalFailureSummary::new("failed to page the collection"));
        ExtractorRejection::Classified(report.with_instance(addressed_uri(parts)))
    })
}

impl<S> FromRequestParts<S> for RequestPagination
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(installed_policy(parts))
    }
}

#[cfg(test)]
mod tests {
    use crate::pagination_policy::RequestPagination;
    use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicy;
    use axum::{
        extract::FromRequestParts,
        http::{Request, StatusCode},
        response::IntoResponse,
    };
    use pretty_assertions::assert_eq;

    /// The policy the assembly installed is the one a handler reads.
    #[tokio::test]
    async fn the_installed_policy_is_extracted() {
        let (mut parts, ()) = Request::get("/ngsi-ld/v1/entities").body(()).expect("a request").into_parts();
        parts.extensions.insert(PaginationPolicy::OffsetDefault);
        let extracted = RequestPagination::from_request_parts(&mut parts, &()).await.expect("installed");
        assert_eq!(extracted, RequestPagination(PaginationPolicy::OffsetDefault));
    }

    /// A request without the policy is a wiring fault, answered as an internal
    /// error rather than paged by an invented default.
    #[tokio::test]
    async fn a_missing_policy_is_an_internal_error() {
        let (mut parts, ()) = Request::get("/ngsi-ld/v1/entities").body(()).expect("a request").into_parts();
        let rejection = RequestPagination::from_request_parts(&mut parts, &()).await.expect_err("not installed");
        assert_eq!(rejection.into_response().status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
