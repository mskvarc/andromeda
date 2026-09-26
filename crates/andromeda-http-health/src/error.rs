//! Focused failures produced by the health binding.

use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_lexicon::identifier_length::IdentifierTooLong;

/// Failures local to health-response construction.
#[derive(Debug, thiserror::Error)]
pub enum HealthHttpError {
    /// The response identifier could not be constructed.
    #[error("health response identifier construction failed")]
    Identifier(#[from] urn_rs::Error),
    /// The response identifier was built longer than any identifier may be.
    #[error("health response identifier exceeds the identifier bound")]
    IdentifierLength(#[from] IdentifierTooLong),
}

/// Classify every health-binding failure without exposing internal details.
#[must_use]
pub fn classify(error: &HealthHttpError) -> ClassifiedReport {
    match error {
        HealthHttpError::Identifier(_) | HealthHttpError::IdentifierLength(_) => {
            ClassifiedReport::internal(error, InternalFailureSummary::IDENTIFIER_RESOLUTION)
        }
    }
}
