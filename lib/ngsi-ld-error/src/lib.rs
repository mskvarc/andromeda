//! NGSI-LD error types and RFC 7807 Problem Details responses for
//! ETSI GS CIM 009 v1.9.1 compliant APIs.
//!
//! This crate provides:
//!
//! - [`NgsiLdErrorType`] — the twelve standard error categories from clause 5.5.2,
//!   each carrying its canonical type URI, human-readable title and HTTP status code.
//! - [`NgsiLdError`] — a concrete error that pairs an [`NgsiLdErrorType`] with a
//!   detail message and request instance URI, and converts directly into an
//!   [Axum](https://docs.rs/axum) JSON response via [`IntoResponse`].
//! - [`types`] — the raw URI constants for use in serialisation (e.g. batch results).

pub mod batch;
pub mod types;

use axum::{
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};
use problem_details::ProblemDetails;
use std::fmt;

/// NGSI-LD error categories as defined in ETSI GS CIM 009 v1.9.1, clause 5.5.2.
///
/// Each variant maps to a canonical `https://uri.etsi.org/ngsi-ld/errors/…` type
/// URI, an HTTP status code, and a short title suitable for Problem Details
/// responses ([RFC 7807]).
///
/// [RFC 7807]: https://www.rfc-editor.org/rfc/rfc7807
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NgsiLdErrorType {
    /// The request was syntactically invalid (400).
    InvalidRequest,
    /// The request body contains invalid or incomplete data (400).
    BadRequestData,
    /// The resource already exists (409).
    AlreadyExists,
    /// A state conflict prevented the operation (409).
    Conflict,
    /// The requested operation is not supported (422).
    OperationNotSupported,
    /// The target resource was not found (404).
    ResourceNotFound,
    /// An unexpected internal error occurred (500).
    InternalError,
    /// The query is too complex to evaluate (403).
    TooComplexQuery,
    /// The query would return too many results (403).
    TooManyResults,
    /// A required JSON-LD `@context` could not be retrieved (503).
    LdContextNotAvailable,
    /// Multi-tenancy is not supported (501).
    NoMultiTenantSupport,
    /// The requested tenant does not exist (404).
    NonexistentTenant,
}

impl NgsiLdErrorType {
    /// Returns the canonical ETSI type URI for this error category.
    pub fn type_uri(&self) -> &'static str {
        match self {
            Self::InvalidRequest => types::INVALID_REQUEST,
            Self::BadRequestData => types::BAD_REQUEST_DATA,
            Self::AlreadyExists => types::ALREADY_EXISTS,
            Self::Conflict => types::CONFLICT,
            Self::OperationNotSupported => types::OPERATION_NOT_SUPPORTED,
            Self::ResourceNotFound => types::RESOURCE_NOT_FOUND,
            Self::InternalError => types::INTERNAL_ERROR,
            Self::TooComplexQuery => types::TOO_COMPLEX_QUERY,
            Self::TooManyResults => types::TOO_MANY_RESULTS,
            Self::LdContextNotAvailable => types::LD_CONTEXT_NOT_AVAILABLE,
            Self::NoMultiTenantSupport => types::NO_MULTI_TENANT_SUPPORT,
            Self::NonexistentTenant => types::NON_EXISTENT_TENANT,
        }
    }

    /// Returns the short human-readable title for this error category.
    pub fn title(&self) -> &'static str {
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

    /// Returns the HTTP status code associated with this error category.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::BadRequestData => StatusCode::BAD_REQUEST,
            Self::AlreadyExists => StatusCode::CONFLICT,
            Self::Conflict => StatusCode::CONFLICT,
            Self::OperationNotSupported => StatusCode::UNPROCESSABLE_ENTITY,
            Self::ResourceNotFound => StatusCode::NOT_FOUND,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::TooComplexQuery => StatusCode::FORBIDDEN,
            Self::TooManyResults => StatusCode::FORBIDDEN,
            Self::LdContextNotAvailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::NoMultiTenantSupport => StatusCode::NOT_IMPLEMENTED,
            Self::NonexistentTenant => StatusCode::NOT_FOUND,
        }
    }
}

impl fmt::Display for NgsiLdErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.title())
    }
}

/// An NGSI-LD API error that serialises as an RFC 7807 Problem Details JSON
/// response when converted into an Axum [`Response`].
///
/// # Example
///
/// ```
/// use axum::http::Uri;
/// use ngsi_ld_error::{NgsiLdError, NgsiLdErrorType};
///
/// let error = NgsiLdError::new(
///     NgsiLdErrorType::ResourceNotFound,
///     "Entity urn:ngsi-ld:Thing:1 not found",
///     Uri::from_static("/ngsi-ld/v1/entities/urn:ngsi-ld:Thing:1"),
/// );
/// ```
#[derive(Debug)]
pub struct NgsiLdError {
    pub error_type: NgsiLdErrorType,
    pub detail: String,
    pub instance: Uri,
}

impl NgsiLdError {
    /// Creates a new error with the given type, detail message and request URI.
    pub fn new(error_type: NgsiLdErrorType, detail: impl Into<String>, instance: Uri) -> Self {
        Self {
            error_type,
            detail: detail.into(),
            instance,
        }
    }
}

impl fmt::Display for NgsiLdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.error_type, self.detail)
    }
}

impl std::error::Error for NgsiLdError {}

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
