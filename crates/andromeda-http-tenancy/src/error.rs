//! Focused failures produced by tenant administration over HTTP.

use andromeda_http_extraction::classification::classify_tenancy_error;
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_tenancy::error::TenancyError;

/// Failures local to the tenancy HTTP binding.
#[derive(Debug, thiserror::Error)]
pub enum TenancyHttpError {
    /// Tenant registry or visibility policy rejected the operation.
    #[error(transparent)]
    Tenancy(#[from] TenancyError),
    /// The tenant-list response identifier could not be constructed.
    #[error("tenant-list response identifier construction failed")]
    Identifier(#[from] urn_rs::Error),
}

/// Classify every tenancy-binding failure exhaustively.
#[must_use]
pub fn classify(error: &TenancyHttpError) -> ClassifiedReport {
    match error {
        TenancyHttpError::Tenancy(source) => classify_tenancy_error(source),
        TenancyHttpError::Identifier(_) => ClassifiedReport::internal(error),
    }
}
