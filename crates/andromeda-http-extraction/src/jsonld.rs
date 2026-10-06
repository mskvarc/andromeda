//! JSON-LD body extractor with context extraction.
//!
//! A body is admitted from its text alone ([`crate::body_admission`]): its
//! JSON values are counted, its numbers checked and its top-level `@context`
//! read without building it. A [`JsonLd`] body is then decoded straight from the
//! text into its domain type, held as a [`SubmittedBody`] until its context is
//! admitted. No handler keeps a request body as an untyped JSON tree: every
//! endpoint names the typed model its body is.

use crate::{
    body_admission::{AdmittedBody, admit_request_body, submitted_context},
    submitted_body::SubmittedBody,
    undecodable_body::UndecodableBody,
};
use andromeda_http_response::{classified_report::ClassifiedReport, extraction_rejection::ExtractorRejection};
use andromeda_jsonld::user_context_admission::SubmittedContext;
use andromeda_ngsi_ld_model::request_body::RequestBody;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::extract::{FromRequest, Request};

/// Axum extractor for NGSI-LD JSON-LD request bodies decoded into a domain type.
///
/// Validates the `Content-Type` header against the supported NGSI-LD media types
/// (`application/json`, `application/ld+json`, `application/merge-patch+json`),
/// extracts and merges `@context` values from the body when present, and
/// decodes the body, its `@context` member skipped, straight from its text into
/// `T`. A body that does not decode is refused only once its context was
/// admitted, so an unretrievable or refused context answers first (clauses
/// 5.5.4 and 5.5.7).
pub struct JsonLd<T> {
    /// The request body with `@context` removed, decoded into `T`.
    pub body: SubmittedBody<T>,
    /// The context the request named in its body or `Link` header, admitted
    /// before anything is read under it.
    pub context: SubmittedContext,
}

impl<S, T> FromRequest<S> for JsonLd<T>
where
    S: Send + Sync,
    T: RequestBody + Send + 'static,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        admit_request_body(req, state, |AdmittedBody { text, context, uri }| {
            let context = submitted_context(context, &text, &uri)?;
            let decoded = T::from_request_text(&text).map_err(|source| {
                let refusal = UndecodableBody::of_text::<T>(&text, source);
                // clone: the refusal names the request, which the held body keeps naming
                ClassifiedReport::public(NgsiLdErrorType::BadRequestData, refusal.to_string()).with_instance(uri.clone())
            });
            Ok(JsonLd {
                body: SubmittedBody::new(decoded, uri),
                context,
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests;
