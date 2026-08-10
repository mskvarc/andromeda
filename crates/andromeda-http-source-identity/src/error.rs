//! Focused failures produced while rendering source identity.

use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;

/// Failures local to the source-identity HTTP binding.
#[derive(Debug, thiserror::Error)]
pub enum SourceIdentityError {
    /// The typed identity could not be serialized for the renderer.
    #[error("context source identity serialization failed")]
    Serialization(#[from] serde_json::Error),
    /// The negotiated response representation could not be rendered.
    #[error("context source identity rendering failed")]
    Rendering(#[from] RenderError),
}

/// Classify every source-identity failure without exposing internal details.
pub fn classify(error: &SourceIdentityError) -> ClassifiedReport {
    match error {
        SourceIdentityError::Serialization(_) | SourceIdentityError::Rendering(_) => {
            let report = ClassifiedReport::internal(error);
            debug_assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
            report
        }
    }
}
