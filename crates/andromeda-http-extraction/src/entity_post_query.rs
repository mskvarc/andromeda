//! The query a POST "Query Entities" carries (clause 6.23.3.1): the clause
//! 5.2.23 body and the URL parameters the body has no member for.
//!
//! Clause 6.23.1 says of the binding that "the behaviour of this clause mirrors
//! the one in clause 6.4.3.2", so once decoded the query is the same request
//! the GET binding reads from its query string. The body is handed over as the
//! clause 5.2.23 Query it is, because a paged result is held by the broker as
//! that Query, so its page pointers can name it.

use crate::{jsonld::JsonLd, query::ValidatedQuery, submitted_body::SubmittedBody};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::user_context_admission::SubmittedContext;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_request::entity_operations::PostQueryParams;
use axum::extract::{FromRequest, FromRequestParts, Request};

/// The clause 5.2.23 body and URL parameters of a POST entity query, with the
/// one context the body extractor resolved (clause 6.3.5).
pub struct EntityPostQuery {
    /// The clause 5.2.23 body, read once the context is admitted.
    pub body: SubmittedBody<Query>,
    /// The URL parameters the body has no member for.
    pub url: PostQueryParams,
    /// The context the query is read against: the body's own `@context` for
    /// `application/ld+json`, the `Link` context for `application/json`.
    pub context: SubmittedContext,
}

impl<S> FromRequest<S> for EntityPostQuery
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(request: Request, state: &S) -> Result<EntityPostQuery, ExtractorRejection> {
        let (mut parts, body) = request.into_parts();
        let ValidatedQuery(url) = ValidatedQuery::<PostQueryParams>::from_request_parts(&mut parts, state).await?;
        let JsonLd { body, context } = JsonLd::<Query>::from_request(Request::from_parts(parts, body), state).await?;
        Ok(EntityPostQuery { body, url, context })
    }
}
