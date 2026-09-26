//! Focused failures produced by component-report response construction.

use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_lexicon::identifier_length::IdentifierTooLong;

/// Failures local to the component-report HTTP binding.
#[derive(Debug, thiserror::Error)]
pub enum ComponentReportError {
    /// The response identifier could not be constructed.
    #[error("component-report response identifier construction failed")]
    Identifier(#[from] urn_rs::Error),
    /// The response identifier was built longer than any identifier may be.
    #[error("component-report response identifier exceeds the identifier bound")]
    IdentifierLength(#[from] IdentifierTooLong),
}

/// Classify every component-report failure without exposing internal details.
#[must_use]
pub fn classify(error: &ComponentReportError) -> ClassifiedReport {
    match error {
        ComponentReportError::Identifier(_) | ComponentReportError::IdentifierLength(_) => {
            ClassifiedReport::internal(error, InternalFailureSummary::IDENTIFIER_RESOLUTION)
        }
    }
}
