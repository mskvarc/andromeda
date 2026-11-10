//! Admitting the request `@context` on every temporal operation that expands
//! one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 speaks of the request's `@context` on
//! every operation that resolves one, so a temporal query and a temporal
//! history read admit it exactly as a temporal write does, through the one
//! admission `andromeda_jsonld::user_context_admission` owns. This module only
//! answers its refusal the way every temporal binding does.
//!
//! The refusal is classified rather than fixed to `BadRequestData`: a context
//! that does not resolve is clause 5.5.6's `LdContextNotAvailable`.

use andromeda_failure_classification::jsonld_failure::classify_jsonld_error;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::{
    engine::processor::JsonLdEngine,
    user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
};
use axum::http::Uri as HttpUri;

/// Admit the request `@context` a temporal operation was submitted with.
///
/// # Errors
///
/// Answers `LdContextNotAvailable` when one of the request's contexts does not
/// resolve, and `BadRequestData` for a user `@context` clause 5.5.7 refuses.
pub(crate) async fn admit_request_context(engine: &JsonLdEngine, submitted: SubmittedContext, instance: &HttpUri) -> Result<AdmittedContext, ProblemResponse> {
    admit_user_context(engine, submitted)
        .await
        .map_err(|error| ProblemResponse::classified(classify_jsonld_error(&error), instance.clone()))
    // clone: the problem details own the instance the borrowed request names
}
