//! Refusing a request `@context` clause 5.5.7 does not admit, on every temporal
//! operation that expands one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 states that the user `@context` "shall
//! not contain JSON-LD Scoped Contexts", because they "could be used to modify
//! terms defined in the Core @context or to reshape NGSI-LD Elements during the
//! expansion of terms". The clause speaks of the request's `@context` on every
//! operation that resolves one, so a temporal query and a temporal history read
//! screen it exactly as a temporal write does.
//!
//! The refusal is classified rather than fixed to `BadRequestData`: the same
//! walk reports a context that does not resolve, which clause 5.5.2 answers with
//! `LdContextNotAvailable`.

use andromeda_http_extraction::classification::classify_jsonld_error;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, scoped_context_admission::reject_scoped_user_contexts};
use axum::http::Uri as HttpUri;

/// Refuse a request `@context` that declares a JSON-LD Scoped Context.
///
/// # Errors
///
/// Answers `BadRequestData` for a user `@context` clause 5.5.7 refuses, and
/// `LdContextNotAvailable` when one of the request's contexts does not resolve.
pub(crate) async fn admit_request_context(engine: &JsonLdEngine, context: &ContextSpec, instance: &HttpUri) -> Result<(), ProblemResponse> {
    reject_scoped_user_contexts(engine, context)
        .await
        .map_err(|error| classify_jsonld_error(&error).with_instance(instance.clone()))
    // clone: the problem details own the instance the borrowed request names
}
