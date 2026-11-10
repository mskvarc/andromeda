//! Focused failures produced by component-report response construction.

use andromeda_failure_classification::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::ComponentReportListIdParseError;

/// Failures local to the component-report HTTP binding.
#[derive(Debug, thiserror::Error)]
pub enum ComponentReportError {
    /// The response identifier could not be minted.
    #[error("component-report response identifier construction failed")]
    Identifier(#[from] ComponentReportListIdParseError),
}

/// Classify every component-report failure without exposing internal details.
#[must_use]
pub fn classify(error: &ComponentReportError) -> ClassifiedReport {
    match error {
        ComponentReportError::Identifier(_) => ClassifiedReport::internal(error, InternalFailureSummary::IDENTIFIER_RESOLUTION),
    }
}
