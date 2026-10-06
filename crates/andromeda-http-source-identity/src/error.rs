//! Focused failures produced while rendering source identity.

use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_render::error::RenderError;

/// Failures local to the source-identity HTTP binding.
#[derive(Debug, thiserror::Error)]
pub enum SourceIdentityError {
    /// The identity could not be written in the negotiated response
    /// representation.
    #[error("context source identity rendering failed")]
    Rendering(#[from] RenderError),
    /// The request context the response names could not be presented or
    /// hosted.
    #[error("context source identity response context failed")]
    ResponseContext(#[from] ResponseContextError),
}

/// Classify every source-identity failure without exposing internal details.
#[must_use]
pub fn classify(error: &SourceIdentityError) -> ClassifiedReport {
    match error {
        SourceIdentityError::Rendering(_) => ClassifiedReport::internal(error, InternalFailureSummary::RESPONSE_RENDERING),
        SourceIdentityError::ResponseContext(source) => classify_response_context_error(source),
    }
}
