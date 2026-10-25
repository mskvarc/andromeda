//! The broker's pagination policy, as a request reads it.
//!
//! The policy is decided once at startup from `[pagination]` and
//! `[extensions]`, and the API assembly installs it as a request extension of
//! every NGSI-LD request. A paged collection reads it through
//! [`RequestPagination`], so no handler can page by a default of its own.
//! It reads beside the policy the `@context` the request is paged under, which
//! the policy holds a followed pointer to and every pointer the page answers
//! with carries (clause 5.5.9.1).

use crate::{addressed_uri::addressed_uri, context::ContextUrl};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    extraction_rejection::ExtractorRejection,
    internal_failure_summary::InternalFailureSummary,
};
use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, policy::PaginationPolicy};
use axum::{extract::FromRequestParts, http::request::Parts};
use iri_rs::IriBuf;
use thiserror::Error;

/// The pagination policy the API assembly installed for this request, and the
/// `@context` the request is paged under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPagination {
    /// The installed policy.
    pub policy: PaginationPolicy,
    /// The context the request's `Link` header named, or the core context.
    pub context: PageContext,
}

/// The request reached a paged collection without the policy installed: a
/// wiring fault of the broker, never a client's.
#[derive(Debug, Error)]
#[error("the request carries no pagination policy; the API assembly installs one on every NGSI-LD request")]
struct PaginationPolicyNotInstalled;

/// The installed policy, or the internal error its absence is.
fn installed_policy(parts: &Parts) -> Result<PaginationPolicy, ExtractorRejection> {
    parts.extensions.get::<PaginationPolicy>().copied().ok_or_else(|| {
        let report = ClassifiedReport::internal(&PaginationPolicyNotInstalled, InternalFailureSummary::new("failed to page the collection"));
        ExtractorRejection::Classified(report.with_instance(addressed_uri(parts)))
    })
}

impl<S> FromRequestParts<S> for RequestPagination
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<RequestPagination, ExtractorRejection> {
        let policy = installed_policy(parts)?;
        let ContextUrl(context) = ContextUrl::from_request_parts(parts, state).await?;
        Ok(RequestPagination {
            policy,
            // clone: the walk's context outlives the request that named it
            context: PageContext::new(context.link_context().cloned().unwrap_or_else(|| IriBuf::from(context.effective_core_url()))),
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::pagination_policy::RequestPagination;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, policy::PaginationPolicy};
    use axum::{
        extract::FromRequestParts,
        http::{Request, StatusCode},
        response::IntoResponse,
    };
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    /// The policy the assembly installed is the one a handler reads.
    #[tokio::test]
    async fn the_installed_policy_is_extracted() {
        let (mut parts, ()) = Request::get("/ngsi-ld/v1/entities").body(()).expect("a request").into_parts();
        parts.extensions.insert(PaginationPolicy::OffsetDefault);
        let extracted = RequestPagination::from_request_parts(&mut parts, &()).await.expect("installed");
        assert_eq!(
            extracted,
            RequestPagination {
                policy: PaginationPolicy::OffsetDefault,
                context: PageContext::new(IriBuf::from(ContextSpec::none().effective_core_url())),
            }
        );
    }

    /// The request is paged under the context its `Link` header named.
    #[tokio::test]
    async fn the_request_is_paged_under_its_link_context() {
        let (mut parts, ()) = Request::get("/ngsi-ld/v1/entities")
            .header(
                "Link",
                r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#,
            )
            .body(())
            .expect("a request")
            .into_parts();
        parts.extensions.insert(PaginationPolicy::OffsetDefault);
        let extracted = RequestPagination::from_request_parts(&mut parts, &()).await.expect("installed");
        assert_eq!(extracted.context.as_str(), "https://example.com/a.jsonld");
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
