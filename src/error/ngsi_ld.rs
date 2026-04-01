//! NGSI-LD error types and RFC 7807 Problem Details responses for
//! ETSI GS CIM 009 v1.9.1 compliant APIs.
//!
//! This module provides:
//!
//! - [`NgsiLdErrorType`] -- the twelve standard error categories from clause 5.5.2,
//!   each carrying its canonical type URI, human-readable title and HTTP status code.
//! - [`NgsiLdError`] -- a concrete error that pairs an [`NgsiLdErrorType`] with a
//!   detail message and request instance URI, and converts directly into an
//!   [Axum](https://docs.rs/axum) JSON response via [`IntoResponse`].

use axum::{
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};
use problem_details::ProblemDetails;
use strum::Display;

use crate::error::AndromedaError;

/// Converts an error into an RFC 7807 Problem Details response.
pub trait ToProblem {
    /// Returns the HTTP status code and a `ProblemDetails` value ready for
    /// serialisation into a JSON response body.
    fn to_problem(&self) -> (StatusCode, ProblemDetails);
}

impl ToProblem for AndromedaError {
    fn to_problem(&self) -> (StatusCode, ProblemDetails) {
        let error_type = self.kind().to_ngsi_ld_error_type();
        let status = error_type.status_code();

        let detail = if self.kind().is_internal() {
            "Internal error".to_string()
        } else {
            self.message().to_string()
        };

        let instance = self.context().instance.clone().unwrap_or_else(|| Uri::from_static("/"));

        let problem = ProblemDetails::new()
            .with_type(Uri::from_static(error_type.type_uri()))
            .with_title(error_type.title())
            .with_status(status)
            .with_detail(detail)
            .with_instance(instance);

        (status, problem)
    }
}

/// NGSI-LD error categories as defined in ETSI GS CIM 009 v1.9.1, clause 5.5.2.
///
/// Each variant maps to a canonical `https://uri.etsi.org/ngsi-ld/errors/...` type
/// URI, an HTTP status code, and a short title suitable for Problem Details
/// responses ([RFC 7807]).
///
/// [RFC 7807]: https://www.rfc-editor.org/rfc/rfc7807
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum NgsiLdErrorType {
    /// The request was syntactically invalid (400).
    #[strum(to_string = "Invalid request")]
    InvalidRequest,
    /// The request body contains invalid or incomplete data (400).
    #[strum(to_string = "Bad request data")]
    BadRequestData,
    /// The resource already exists (409).
    #[strum(to_string = "Already exists")]
    AlreadyExists,
    /// A state conflict prevented the operation (409).
    #[strum(to_string = "Conflict")]
    Conflict,
    /// The requested operation is not supported (422).
    #[strum(to_string = "Operation not supported")]
    OperationNotSupported,
    /// The target resource was not found (404).
    #[strum(to_string = "Resource not found")]
    ResourceNotFound,
    /// An unexpected internal error occurred (500).
    #[strum(to_string = "Internal error")]
    InternalError,
    /// The query is too complex to evaluate (403).
    #[strum(to_string = "Too complex query")]
    #[allow(dead_code)]
    TooComplexQuery,
    /// The query would return too many results (403).
    #[strum(to_string = "Too many results")]
    #[allow(dead_code)]
    TooManyResults,
    /// A required JSON-LD `@context` could not be retrieved (503).
    #[strum(to_string = "LD context not available")]
    LdContextNotAvailable,
    /// Multi-tenancy is not supported (501).
    #[strum(to_string = "No multi-tenant support")]
    NoMultiTenantSupport,
    /// The requested tenant does not exist (404).
    #[strum(to_string = "Nonexistent tenant")]
    NonexistentTenant,
}

impl NgsiLdErrorType {
    /// NGSI-LD error type URI constants (ETSI GS CIM 009 v1.9.1, clause 5.5.2).
    pub const INVALID_REQUEST_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";
    pub const BAD_REQUEST_DATA_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";
    pub const ALREADY_EXISTS_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists";
    pub const CONFLICT_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/Conflict";
    pub const OPERATION_NOT_SUPPORTED_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported";
    pub const RESOURCE_NOT_FOUND_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";
    pub const INTERNAL_ERROR_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/InternalError";
    pub const TOO_COMPLEX_QUERY_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/TooComplexQuery";
    pub const TOO_MANY_RESULTS_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/TooManyResults";
    pub const LD_CONTEXT_NOT_AVAILABLE_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable";
    pub const NO_MULTI_TENANT_SUPPORT_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/NoMultiTenantSupport";
    pub const NON_EXISTENT_TENANT_URI: &str = "https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant";

    /// Returns the canonical ETSI type URI for this error category.
    pub const fn type_uri(self) -> &'static str {
        match self {
            Self::InvalidRequest => Self::INVALID_REQUEST_URI,
            Self::BadRequestData => Self::BAD_REQUEST_DATA_URI,
            Self::AlreadyExists => Self::ALREADY_EXISTS_URI,
            Self::Conflict => Self::CONFLICT_URI,
            Self::OperationNotSupported => Self::OPERATION_NOT_SUPPORTED_URI,
            Self::ResourceNotFound => Self::RESOURCE_NOT_FOUND_URI,
            Self::InternalError => Self::INTERNAL_ERROR_URI,
            Self::TooComplexQuery => Self::TOO_COMPLEX_QUERY_URI,
            Self::TooManyResults => Self::TOO_MANY_RESULTS_URI,
            Self::LdContextNotAvailable => Self::LD_CONTEXT_NOT_AVAILABLE_URI,
            Self::NoMultiTenantSupport => Self::NO_MULTI_TENANT_SUPPORT_URI,
            Self::NonexistentTenant => Self::NON_EXISTENT_TENANT_URI,
        }
    }

    /// Returns the short human-readable title for this error category.
    pub const fn title(self) -> &'static str {
        match self {
            Self::InvalidRequest => "Invalid request",
            Self::BadRequestData => "Bad request data",
            Self::AlreadyExists => "Already exists",
            Self::Conflict => "Conflict",
            Self::OperationNotSupported => "Operation not supported",
            Self::ResourceNotFound => "Resource not found",
            Self::InternalError => "Internal error",
            Self::TooComplexQuery => "Too complex query",
            Self::TooManyResults => "Too many results",
            Self::LdContextNotAvailable => "LD context not available",
            Self::NoMultiTenantSupport => "No multi-tenant support",
            Self::NonexistentTenant => "Nonexistent tenant",
        }
    }

    /// Returns the HTTP status code as a `u16` for this error category.
    pub const fn status_u16(self) -> u16 {
        match self {
            Self::InvalidRequest => 400,
            Self::BadRequestData => 400,
            Self::AlreadyExists => 409,
            Self::Conflict => 409,
            Self::OperationNotSupported => 422,
            Self::ResourceNotFound => 404,
            Self::InternalError => 500,
            Self::TooComplexQuery => 403,
            Self::TooManyResults => 403,
            Self::LdContextNotAvailable => 503,
            Self::NoMultiTenantSupport => 501,
            Self::NonexistentTenant => 404,
        }
    }

    /// Returns the HTTP [`StatusCode`] associated with this error category.
    pub fn status_code(self) -> StatusCode {
        // SAFETY: all values are hardcoded valid HTTP status codes.
        StatusCode::from_u16(self.status_u16()).expect("hardcoded valid HTTP status code")
    }
}

/// An NGSI-LD API error that serialises as an RFC 7807 Problem Details JSON
/// response when converted into an Axum [`Response`].
///
/// # Example
///
/// ```ignore
/// use axum::http::Uri;
/// use crate::error::ngsi_ld::{NgsiLdError, NgsiLdErrorType};
///
/// let error = NgsiLdError::new(
///     NgsiLdErrorType::ResourceNotFound,
///     "Entity urn:ngsi-ld:Thing:1 not found",
///     Uri::from_static("/ngsi-ld/v1/entities/urn:ngsi-ld:Thing:1"),
/// );
/// ```
#[derive(Debug, thiserror::Error)]
#[error("{error_type}: {detail}")]
pub struct NgsiLdError {
    pub error_type: NgsiLdErrorType,
    pub detail: String,
    pub instance: Uri,
}

impl NgsiLdError {
    /// Creates a new error with the given type, detail message and request URI.
    pub fn new(error_type: NgsiLdErrorType, detail: impl Into<String>, instance: Uri) -> NgsiLdError {
        NgsiLdError {
            error_type,
            detail: detail.into(),
            instance,
        }
    }
}

impl IntoResponse for NgsiLdError {
    fn into_response(self) -> Response {
        let status = self.error_type.status_code();

        let problem = ProblemDetails::new()
            .with_type(Uri::from_static(self.error_type.type_uri()))
            .with_title(self.error_type.title())
            .with_status(status)
            .with_detail(self.detail)
            .with_instance(self.instance);

        (status, axum::Json(problem)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, response::IntoResponse};
    use serde_json::Value;

    #[test]
    fn type_uri_round_trips_through_title() {
        let variants = [
            NgsiLdErrorType::InvalidRequest,
            NgsiLdErrorType::BadRequestData,
            NgsiLdErrorType::AlreadyExists,
            NgsiLdErrorType::Conflict,
            NgsiLdErrorType::OperationNotSupported,
            NgsiLdErrorType::ResourceNotFound,
            NgsiLdErrorType::InternalError,
            NgsiLdErrorType::TooComplexQuery,
            NgsiLdErrorType::TooManyResults,
            NgsiLdErrorType::LdContextNotAvailable,
            NgsiLdErrorType::NoMultiTenantSupport,
            NgsiLdErrorType::NonexistentTenant,
        ];
        for v in &variants {
            assert!(v.type_uri().starts_with("https://uri.etsi.org/ngsi-ld/errors/"));
            assert!(!v.title().is_empty());
        }
    }

    #[test]
    fn display_uses_title() {
        assert_eq!(NgsiLdErrorType::ResourceNotFound.to_string(), "Resource not found");
    }

    #[test]
    fn error_display_combines_type_and_detail() {
        let err = NgsiLdError::new(NgsiLdErrorType::BadRequestData, "missing id", Uri::from_static("/"));
        assert_eq!(err.to_string(), "Bad request data: missing id");
    }

    #[tokio::test]
    async fn into_response_produces_problem_json() {
        let err = NgsiLdError::new(NgsiLdErrorType::ResourceNotFound, "not found", Uri::from_static("/ngsi-ld/v1/entities/x"));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], 404);
        assert_eq!(json["title"], "Resource not found");
        assert_eq!(json["instance"], "/ngsi-ld/v1/entities/x");
    }
}
