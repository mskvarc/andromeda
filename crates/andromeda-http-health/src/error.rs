//! Focused failures produced by the health binding.

use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::HealthCheckIdParseError;

/// Failures local to health-response construction.
#[derive(Debug, thiserror::Error)]
pub enum HealthHttpError {
    /// The response identifier could not be minted.
    #[error("health response identifier construction failed")]
    Identifier(#[from] HealthCheckIdParseError),
}

/// Classify every health-binding failure without exposing internal details.
#[must_use]
pub fn classify(error: &HealthHttpError) -> ClassifiedReport {
    match error {
        HealthHttpError::Identifier(_) => ClassifiedReport::internal(error, InternalFailureSummary::IDENTIFIER_RESOLUTION),
    }
}
