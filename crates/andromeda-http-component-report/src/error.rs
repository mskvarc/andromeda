//! Focused failures produced by component-report response construction.

use andromeda_http_response::classified_report::ClassifiedReport;

/// Failures local to the component-report HTTP binding.
#[derive(Debug, thiserror::Error)]
pub enum ComponentReportError {
    /// The response identifier could not be constructed.
    #[error("component-report response identifier construction failed")]
    Identifier(#[from] urn_rs::Error),
}

/// Classify every component-report failure without exposing internal details.
pub fn classify(error: &ComponentReportError) -> ClassifiedReport {
    match error {
        ComponentReportError::Identifier(_) => ClassifiedReport::internal(error),
    }
}
