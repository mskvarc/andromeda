//! Response adapter for focused tenant-state extraction failures.

use crate::{extraction_rejection::ExtractorRejection, problem_response::ProblemResponse};
use axum::response::{IntoResponse, Response};

/// Failure while extracting a tenant-scoped state value.
pub enum TenantStateRejection {
    /// Typed tenant-header extraction failed.
    Extraction(ExtractorRejection),
    /// The binding-specific tenant resolver rejected the request.
    Resolution(ProblemResponse),
}

impl IntoResponse for TenantStateRejection {
    fn into_response(self) -> Response {
        match self {
            TenantStateRejection::Extraction(rejection) => rejection.into_response(),
            TenantStateRejection::Resolution(rejection) => rejection.into_response(),
        }
    }
}
