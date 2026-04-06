//! Extractor rejection types for NGSI-LD API error responses.
//!
//! Maps extractor-level failures to the appropriate NGSI-LD error types
//! (`InvalidRequest`, `BadRequestData`) as required by the specification.

use crate::error::ngsi_ld::{NgsiLdError, NgsiLdErrorType};
use axum::{
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};

/// Rejection type for NGSI-LD API extractors.
///
/// Converts extraction failures into spec-compliant NGSI-LD error responses
/// with the correct HTTP status code and problem detail body.
#[derive(Debug)]
pub enum ExtractorRejection {
    /// Empty response with only a status code (e.g., 406 Not Acceptable).
    Empty(StatusCode),
    /// NGSI-LD `InvalidRequest` error (400).
    InvalidRequest {
        /// Human-readable error detail.
        detail: String,
        /// The request URI that triggered the error.
        instance: Uri,
    },
    /// NGSI-LD `BadRequestData` error (400).
    BadRequestData {
        /// Human-readable error detail.
        detail: String,
        /// The request URI that triggered the error.
        instance: Uri,
    },
}

impl ExtractorRejection {
    /// Create an `InvalidRequest` rejection.
    pub fn invalid_request(detail: impl Into<String>, instance: Uri) -> ExtractorRejection {
        ExtractorRejection::InvalidRequest {
            detail: detail.into(),
            instance,
        }
    }

    /// Create a `BadRequestData` rejection.
    pub fn bad_request_data(detail: impl Into<String>, instance: Uri) -> ExtractorRejection {
        ExtractorRejection::BadRequestData {
            detail: detail.into(),
            instance,
        }
    }
}

impl IntoResponse for ExtractorRejection {
    fn into_response(self) -> Response {
        match self {
            ExtractorRejection::Empty(status) => status.into_response(),
            ExtractorRejection::InvalidRequest { detail, instance } => {
                let ngsi = NgsiLdError::new(NgsiLdErrorType::InvalidRequest, detail, instance);
                ngsi.into_response()
            }
            ExtractorRejection::BadRequestData { detail, instance } => {
                let ngsi = NgsiLdError::new(NgsiLdErrorType::BadRequestData, detail, instance);
                ngsi.into_response()
            }
        }
    }
}
