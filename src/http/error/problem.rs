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

use crate::core::error::{AndromedaError, ErrorKind};
use axum::{
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};
use problem_details::ProblemDetails;
use strum::{Display, IntoStaticStr};

/// NGSI-LD error categories as defined in ETSI GS CIM 009 v1.9.1, clause 5.5.2.
///
/// Each variant maps to a canonical `https://uri.etsi.org/ngsi-ld/errors/...` type
/// URI, an HTTP status code, and a short title suitable for Problem Details
/// responses ([RFC 7807]).
///
/// [RFC 7807]: https://www.rfc-editor.org/rfc/rfc7807
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
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

    /// Resolves an ETSI type URI back to its [`NgsiLdErrorType`] variant.
    ///
    /// Returns `None` if the URI does not match any known error category.
    pub fn from_type_uri(uri: &str) -> Option<NgsiLdErrorType> {
        match uri {
            Self::INVALID_REQUEST_URI => Some(NgsiLdErrorType::InvalidRequest),
            Self::BAD_REQUEST_DATA_URI => Some(NgsiLdErrorType::BadRequestData),
            Self::ALREADY_EXISTS_URI => Some(NgsiLdErrorType::AlreadyExists),
            Self::CONFLICT_URI => Some(NgsiLdErrorType::Conflict),
            Self::OPERATION_NOT_SUPPORTED_URI => Some(NgsiLdErrorType::OperationNotSupported),
            Self::RESOURCE_NOT_FOUND_URI => Some(NgsiLdErrorType::ResourceNotFound),
            Self::INTERNAL_ERROR_URI => Some(NgsiLdErrorType::InternalError),
            Self::TOO_COMPLEX_QUERY_URI => Some(NgsiLdErrorType::TooComplexQuery),
            Self::TOO_MANY_RESULTS_URI => Some(NgsiLdErrorType::TooManyResults),
            Self::LD_CONTEXT_NOT_AVAILABLE_URI => Some(NgsiLdErrorType::LdContextNotAvailable),
            Self::NO_MULTI_TENANT_SUPPORT_URI => Some(NgsiLdErrorType::NoMultiTenantSupport),
            Self::NON_EXISTENT_TENANT_URI => Some(NgsiLdErrorType::NonexistentTenant),
            _ => None,
        }
    }

    /// Returns the HTTP [`StatusCode`] associated with this error category.
    ///
    /// All status codes are hardcoded valid HTTP values, so the conversion is
    /// infallible in practice. Returns `500 Internal Server Error` in the
    /// unreachable case of an invalid code.
    pub const fn status_code(self) -> StatusCode {
        match StatusCode::from_u16(match self {
            Self::InvalidRequest | Self::BadRequestData => 400,
            Self::AlreadyExists | Self::Conflict => 409,
            Self::OperationNotSupported => 422,
            Self::ResourceNotFound | Self::NonexistentTenant => 404,
            Self::InternalError => 500,
            Self::TooComplexQuery | Self::TooManyResults => 403,
            Self::LdContextNotAvailable => 503,
            Self::NoMultiTenantSupport => 501,
        }) {
            Ok(code) => code,
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<&ErrorKind> for NgsiLdErrorType {
    fn from(kind: &ErrorKind) -> NgsiLdErrorType {
        match kind {
            ErrorKind::EntityNotFound(_)
            | ErrorKind::SubscriptionNotFound(_)
            | ErrorKind::CsrNotFound(_)
            | ErrorKind::SnapshotNotFound(_)
            | ErrorKind::ContextNotFound(_)
            | ErrorKind::EntityMapNotFound(_)
            | ErrorKind::TemporalNotFound(_) => NgsiLdErrorType::ResourceNotFound,

            ErrorKind::EntityAlreadyExists(_)
            | ErrorKind::SubscriptionAlreadyExists(_)
            | ErrorKind::CsrAlreadyExists(_)
            | ErrorKind::SnapshotAlreadyExists(_)
            | ErrorKind::DatabaseUniqueViolation(_) => NgsiLdErrorType::AlreadyExists,

            ErrorKind::Conflict(_) => NgsiLdErrorType::Conflict,
            ErrorKind::OperationNotSupported(_) => NgsiLdErrorType::OperationNotSupported,
            ErrorKind::ContextNotAvailable(_) => NgsiLdErrorType::LdContextNotAvailable,
            ErrorKind::NonexistentTenant(_) => NgsiLdErrorType::NonexistentTenant,
            ErrorKind::MultiTenancyNotSupported => NgsiLdErrorType::NoMultiTenantSupport,

            ErrorKind::JsonLdProcessing(_)
            | ErrorKind::SubscriptionValidation(_)
            | ErrorKind::EntityValidation(_)
            | ErrorKind::TemporalValidation(_)
            | ErrorKind::CsrValidation(_)
            | ErrorKind::SnapshotValidation(_)
            | ErrorKind::EntityMapValidation(_)
            | ErrorKind::QueryValidation(_)
            | ErrorKind::JsonParse(_)
            | ErrorKind::InvalidUrn(_)
            | ErrorKind::DatabaseRegexError(_) => NgsiLdErrorType::BadRequestData,

            ErrorKind::MalformedRequest(_) => NgsiLdErrorType::InvalidRequest,

            ErrorKind::Database(_)
            | ErrorKind::Migration(_)
            | ErrorKind::Config(_)
            | ErrorKind::TracingInit(_)
            | ErrorKind::Io(_)
            | ErrorKind::ExternalRequest(_)
            | ErrorKind::Internal(_) => NgsiLdErrorType::InternalError,
        }
    }
}

/// An NGSI-LD API error that serialises as an RFC 7807 Problem Details JSON
/// response when converted into an Axum [`Response`].
///
/// # Example
///
/// ```ignore
/// use axum::http::Uri;
/// use crate::http::error::problem::{NgsiLdError, NgsiLdErrorType};
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

impl From<AndromedaError> for NgsiLdError {
    fn from(err: AndromedaError) -> NgsiLdError {
        let error_type: NgsiLdErrorType = err.kind().into();
        let instance = err.instance().cloned().unwrap_or_else(|| Uri::from_static("/"));

        if err.is_internal() {
            tracing::error!(
                error = %err,
                kind = ?err.kind(),
                "Internal error (sanitized before sending to client)"
            );
        }

        let detail = if err.is_internal() { "Internal error".to_string() } else { err.to_string() };

        NgsiLdError::new(error_type, detail, instance)
    }
}

impl IntoResponse for NgsiLdError {
    fn into_response(self) -> Response {
        let status = self.error_type.status_code();

        let problem = ProblemDetails::new()
            .with_type(Uri::from_static(self.error_type.type_uri()))
            .with_title(Into::<&'static str>::into(self.error_type))
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
            assert!(!Into::<&'static str>::into(*v).is_empty());
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

    #[test]
    fn malformed_request_maps_to_invalid_request() {
        let err = AndromedaError::malformed_request("bad");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.error_type, NgsiLdErrorType::InvalidRequest);
    }

    #[tokio::test]
    async fn response_includes_instance_and_status() {
        let err = AndromedaError::malformed_request("bad request").with_instance(Uri::from_static("/ngsi-ld/v1/entities"));
        let ngsi: NgsiLdError = err.into();
        let response = ngsi.into_response();
        assert_eq!(response.status().as_u16(), 400);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("instance").and_then(|v| v.as_str()), Some("/ngsi-ld/v1/entities"));
        assert_eq!(json.get("status").and_then(|v| v.as_u64()), Some(400));
    }

    #[test]
    fn internal_errors_are_sanitized() {
        let err = AndromedaError::internal("secret");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.detail, "Internal error");
    }

    #[test]
    fn internal_error_is_sanitized() {
        let err = AndromedaError::internal("upstream timeout");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.error_type, NgsiLdErrorType::InternalError);
        assert_eq!(ngsi.detail, "Internal error");
    }

    #[test]
    fn error_kind_to_ngsi_ld_error_type_mapping() {
        assert_eq!(NgsiLdErrorType::from(&ErrorKind::MalformedRequest("x".into())), NgsiLdErrorType::InvalidRequest);
        assert_eq!(NgsiLdErrorType::from(&ErrorKind::EntityValidation("x".into())), NgsiLdErrorType::BadRequestData);
        assert_eq!(NgsiLdErrorType::from(&ErrorKind::EntityNotFound("x".into())), NgsiLdErrorType::ResourceNotFound);
        assert_eq!(
            NgsiLdErrorType::from(&ErrorKind::EntityAlreadyExists("x".into())),
            NgsiLdErrorType::AlreadyExists
        );
        assert_eq!(NgsiLdErrorType::from(&ErrorKind::Conflict("x".into())), NgsiLdErrorType::Conflict);
        assert_eq!(
            NgsiLdErrorType::from(&ErrorKind::ContextNotAvailable("x".into())),
            NgsiLdErrorType::LdContextNotAvailable
        );
        assert_eq!(
            NgsiLdErrorType::from(&ErrorKind::MultiTenancyNotSupported),
            NgsiLdErrorType::NoMultiTenantSupport
        );
        assert_eq!(
            NgsiLdErrorType::from(&ErrorKind::NonexistentTenant("x".into())),
            NgsiLdErrorType::NonexistentTenant
        );
        assert_eq!(
            NgsiLdErrorType::from(&ErrorKind::OperationNotSupported("x".into())),
            NgsiLdErrorType::OperationNotSupported
        );
        assert_eq!(NgsiLdErrorType::from(&ErrorKind::Internal("x".into())), NgsiLdErrorType::InternalError);
    }
}
