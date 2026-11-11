//! The broker's pagination policy, as a request reads it.
//!
//! The policy is decided once at startup from `[pagination]` and
//! `[extensions]`, and the API assembly installs it as a request extension of
//! every NGSI-LD request. A paged collection reads it through
//! [`RequestPagination`], so no handler can page by a default of its own.
//! It reads beside the policy the `@context` the request is paged under, which
//! the policy holds a followed pointer to and every pointer the page answers
//! with carries (clause 5.5.9.1), the digest of the query it names, which
//! the policy holds a followed cursor to, and the request as a client reaches
//! it through the configured `apiRoot`, which every pointer rebuilds.

use crate::{addressed_uri::addressed_uri, api_root::ConfiguredApiRoot, context::ContextUrl};
use andromeda_failure_classification::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_http_response::{
    extraction_rejection::ExtractorRejection,
    link_target::{NotAPostedQuery, PointedRequest},
    problem_response::ProblemResponse,
};
use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, page_query::PageQuery, policy::PaginationPolicy};
use axum::{extract::FromRequestParts, http::request::Parts};
use iri_rs::IriBuf;
use thiserror::Error;

/// The pagination policy the API assembly installed for this request, the
/// `@context` and query the request is paged under, and the request its
/// pointers rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPagination {
    /// The installed policy.
    pub policy: PaginationPolicy,
    /// The context the request's `Link` header named, or the core context.
    pub context: PageContext,
    /// The digest of the query the request's URI names.
    pub query: PageQuery,
    /// The request as a client reaches it through the configured `apiRoot`.
    pub pointed: PointedRequest,
}

impl RequestPagination {
    /// The pagination of a query sent by POST that the broker holds under
    /// `key`: paged under the key, its pointers rebuilt on the GET binding of
    /// the same query.
    ///
    /// # Errors
    ///
    /// [`NotAPostedQuery`] for a request addressed to no POST query binding.
    pub fn posted(self, key: PageQuery) -> Result<RequestPagination, NotAPostedQuery> {
        Ok(RequestPagination {
            query: key,
            pointed: self.pointed.into_query_binding()?,
            ..self
        })
    }

    /// The pagination of a followed pointer naming the posted query `key`:
    /// paged under the key, which stands for the query and its `@context`.
    #[must_use]
    pub fn following(self, key: PageQuery) -> RequestPagination {
        RequestPagination { query: key, ..self }
    }
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
        ExtractorRejection::Classified(ProblemResponse::classified(report, addressed_uri(parts)))
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
            query: PageQuery::of(parts.uri.query()),
            pointed: PointedRequest::new(
                // clone: the configured URL stays in the request's extensions for later extractors
                parts.extensions.get::<ConfiguredApiRoot>().map(|root| root.url().clone()),
                addressed_uri(parts),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{api_root::ConfiguredApiRoot, pagination_policy::RequestPagination};
    use andromeda_http_response::link_target::PointedRequest;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, page_query::PageQuery, policy::PaginationPolicy};
    use axum::{
        extract::{FromRequestParts, OriginalUri},
        http::{Request, StatusCode, Uri},
        response::IntoResponse,
    };
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use url::Url;

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
                query: PageQuery::of(None),
                pointed: PointedRequest::new(None, Uri::from_static("/ngsi-ld/v1/entities")),
            }
        );
    }

    /// The pointers rebuild the URI the client addressed below the configured
    /// `apiRoot`, not the URI a nested router rewrote.
    #[tokio::test]
    async fn the_pointers_rebuild_the_addressed_uri_under_the_configured_api_root() {
        let (mut parts, ()) = Request::get("/entities?type=Vehicle").body(()).expect("a request").into_parts();
        parts.extensions.insert(PaginationPolicy::OffsetDefault);
        parts.extensions.insert(OriginalUri(Uri::from_static("/ngsi-ld/v1/entities?type=Vehicle")));
        let api_root = Url::parse("https://proxy.example/context-broker").expect("a valid URL");
        parts.extensions.insert(ConfiguredApiRoot::new(api_root.clone()));
        let extracted = RequestPagination::from_request_parts(&mut parts, &()).await.expect("installed");
        assert_eq!(
            extracted.pointed,
            PointedRequest::new(Some(api_root), Uri::from_static("/ngsi-ld/v1/entities?type=Vehicle"))
        );
    }

    /// The request is paged under the digest of the query its URI names.
    #[tokio::test]
    async fn the_request_is_paged_under_the_digest_of_its_query() {
        let (mut parts, ()) = Request::get("/ngsi-ld/v1/entities?type=Vehicle&limit=2")
            .body(())
            .expect("a request")
            .into_parts();
        parts.extensions.insert(PaginationPolicy::OffsetDefault);
        let extracted = RequestPagination::from_request_parts(&mut parts, &()).await.expect("installed");
        assert_eq!(extracted.query, PageQuery::of(Some("type=Vehicle")));
    }

    /// A query sent by POST is paged under its key, and its pointers rebuild
    /// the GET binding of the same query; a followed pointer is paged under
    /// the key it names.
    #[tokio::test]
    async fn a_posted_query_is_paged_under_its_key_on_the_get_binding() {
        let (mut parts, ()) = Request::post("/ngsi-ld/v1/entityOperations/query?limit=2")
            .body(())
            .expect("a request")
            .into_parts();
        parts.extensions.insert(PaginationPolicy::OffsetDefault);
        let extracted = RequestPagination::from_request_parts(&mut parts, &()).await.expect("installed");
        let key = PageQuery::of(Some("type=Posted"));
        let posted = extracted.clone().posted(key.clone()).expect("a POST query binding");
        assert_eq!(posted.query, key);
        assert_eq!(posted.pointed, PointedRequest::new(None, Uri::from_static("/ngsi-ld/v1/entities?limit=2")));
        assert_eq!(extracted.following(key.clone()).query, key);
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
