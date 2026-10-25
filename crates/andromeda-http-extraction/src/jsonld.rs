//! JSON-LD body extractor with context extraction.
//!
//! A body is admitted from its text alone ([`crate::body_admission`]): its
//! JSON values are counted, its numbers checked and its top-level `@context`
//! read without building it. A [`JsonLd`] body is held as a [`SubmittedBody`]
//! until its context is admitted, then decoded straight from the text into its
//! domain type under the core term aliases of that context. No handler keeps a
//! request body as an untyped JSON tree: every endpoint names the typed model
//! its body is.

use crate::{
    body_admission::{AdmittedBody, admit_request_body, submitted_context},
    submitted_body::SubmittedBody,
    undecodable_body::UndecodableBody,
};
use andromeda_http_response::{classified_report::ClassifiedReport, extraction_rejection::ExtractorRejection, problem_response::ProblemResponse};
use andromeda_jsonld::user_context_admission::SubmittedContext;
use andromeda_ngsi_ld_model::request_body::RequestBody;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
use axum::{
    extract::{FromRequest, Request},
    http::Uri,
};

/// Axum extractor for NGSI-LD JSON-LD request bodies decoded into a domain type.
///
/// Validates the `Content-Type` header against the supported NGSI-LD media types
/// (`application/json`, `application/ld+json`, `application/merge-patch+json`),
/// extracts and merges `@context` values from the body when present, and
/// holds the admitted text until the context is admitted. The body is then
/// decoded, its `@context` member skipped, straight from its text into `T`,
/// each member read as the core term the context says it stands for, so an
/// unretrievable or refused context answers before the body is judged
/// (clauses 5.5.4 and 5.5.7).
pub struct JsonLd<T> {
    /// The request body, decoded into `T` once its context is admitted.
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
        admit_request_body(
            req,
            state,
            |AdmittedBody {
                 text,
                 context,
                 uri,
                 blocking_decode,
             }| {
                let context = submitted_context(context, &text, &uri)?;
                Ok(JsonLd {
                    body: SubmittedBody::new(text, uri, blocking_decode, read_request_body::<T>),
                    context,
                })
            },
        )
        .await
    }
}

/// Read the request body `text` as a `T` through `aliases`, a body that does
/// not decode refused as `BadRequestData` naming the member it failed at.
///
/// # Errors
///
/// Returns the `BadRequestData` of a body that does not decode.
fn read_request_body<T: RequestBody>(text: &[u8], aliases: &CoreTermAliases, uri: &Uri) -> Result<T, ProblemResponse> {
    T::from_request_text(text, aliases).map_err(|source| {
        let refusal = UndecodableBody::of_text::<T>(text, aliases, source);
        // clone: the refusal owns the URI it names
        ClassifiedReport::public(NgsiLdErrorType::BadRequestData, refusal.to_string()).with_instance(uri.clone())
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
