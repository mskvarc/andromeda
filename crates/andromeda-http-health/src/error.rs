//! Focused failures produced by the health binding.

use andromeda_http_response::classified_report::ClassifiedReport;

/// Failures local to health-response construction.
#[derive(Debug, thiserror::Error)]
pub enum HealthHttpError {
    /// The response identifier could not be constructed.
    #[error("health response identifier construction failed")]
    Identifier(#[from] urn_rs::Error),
}

/// Classify every health-binding failure without exposing internal details.
pub fn classify(error: &HealthHttpError) -> ClassifiedReport {
    match error {
        HealthHttpError::Identifier(_) => ClassifiedReport::internal(error),
    }
}
