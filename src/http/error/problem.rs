//! The RFC 7807 problem-details response an NGSI-LD failure becomes.
//!
//! [`NgsiLdError`] pairs a clause 5.5.2 category with a detail message and the
//! request URI it happened on, and converts directly into an
//! [Axum](https://docs.rs/axum) JSON response. It stays in the binary because
//! it holds an `axum::http::Uri` and implements [`IntoResponse`]: both ends of
//! that conversion are the web framework's, and keeping the impl local is what
//! keeps the orphan rule from ever being engaged.

use crate::{error::AndromedaError, http::error::category};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
use axum::{
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};
use problem_details::ProblemDetails;

/// An NGSI-LD API error, ready to be answered as RFC 7807 problem details.
///
/// # Example
///
/// ```ignore
/// let error = NgsiLdError::new(
///     NgsiLdErrorType::ResourceNotFound,
///     "Entity urn:ngsi-ld:Thing:1 not found",
///     Uri::from_static("/ngsi-ld/v1/entities/urn:ngsi-ld:Thing:1"),
/// );
/// ```
#[derive(Debug, thiserror::Error)]
#[error("{error_type}: {detail}")]
pub struct NgsiLdError {
    /// The clause 5.5.2 category, which fixes the type IRI, title and status.
    pub error_type: NgsiLdErrorType,
    /// The human-readable explanation sent to the client.
    pub detail: String,
    /// The request URI the failure happened on.
    pub instance: Uri,
    /// HTTP status selected for this occurrence.
    pub status: StatusCode,
}

impl NgsiLdError {
    /// Report `error_type` against `instance`, with a human-readable detail.
    pub fn new(error_type: NgsiLdErrorType, detail: impl Into<String>, instance: Uri) -> NgsiLdError {
        NgsiLdError {
            error_type,
            detail: detail.into(),
            instance,
            status: error_type.status_code(),
        }
    }
}

impl From<AndromedaError> for NgsiLdError {
    /// Answer a broker failure as a problem detail.
    ///
    /// An internal failure is logged in full and reported as the bare phrase
    /// "Internal error": clause 5.5.2 has nothing to say about what went wrong
    /// inside a broker, and the message may name a table, a query or a host.
    fn from(error: AndromedaError) -> NgsiLdError {
        let error_type = category::of(error.kind());
        let instance = error.instance().cloned().unwrap_or_else(|| Uri::from_static("/"));

        if error.is_internal() {
            tracing::error!(
                error = %error,
                kind = ?error.kind(),
                "Internal error (sanitized before sending to client)"
            );
            return NgsiLdError::new(error_type, "Internal error", instance);
        }

        let status = match error.kind() {
            crate::error::ErrorKind::ContextSourceGateway(_) => StatusCode::BAD_GATEWAY,
            _other => error_type.status_code(),
        };
        let detail = error.to_string();
        let mut result = NgsiLdError::new(error_type, detail, instance);
        result.status = status;
        result
    }
}

impl From<RenderError> for NgsiLdError {
    /// Answer a rendering failure as a problem detail.
    ///
    /// Routed through [`AndromedaError`] rather than mapped here, so a
    /// rendering failure is categorised, logged and sanitised exactly as every
    /// other internal failure is. The impl exists because `?` applies one
    /// `From`, not two.
    fn from(error: RenderError) -> NgsiLdError {
        NgsiLdError::from(AndromedaError::from(error))
    }
}

impl IntoResponse for NgsiLdError {
    fn into_response(self) -> Response {
        let status = self.status;

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
    use crate::{error::AndromedaError, http::error::problem::NgsiLdError};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        body::to_bytes,
        http::{StatusCode, Uri},
        response::IntoResponse,
    };
    use serde_json::Value;

    #[test]
    fn an_error_renders_as_its_title_and_detail() {
        let error = NgsiLdError::new(NgsiLdErrorType::BadRequestData, "missing id", Uri::from_static("/"));
        pretty_assertions::assert_eq!(error.to_string(), "Bad request data: missing id");
    }

    #[tokio::test]
    async fn the_response_carries_the_category_status_title_and_instance() {
        let response = NgsiLdError::new(NgsiLdErrorType::ResourceNotFound, "not found", Uri::from_static("/ngsi-ld/v1/entities/x")).into_response();
        pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let problem: Value = serde_json::from_slice(&body).expect("problem details");
        pretty_assertions::assert_eq!(problem["status"], 404);
        pretty_assertions::assert_eq!(problem["title"], "Resource not found");
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/entities/x");
    }

    #[tokio::test]
    async fn a_broker_failure_keeps_the_instance_it_was_tagged_with() {
        let error = AndromedaError::malformed_request("bad request").with_instance(&Uri::from_static("/ngsi-ld/v1/entities"));
        let response = NgsiLdError::from(error).into_response();
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let problem: Value = serde_json::from_slice(&body).expect("problem details");
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/entities");
    }

    #[test]
    fn a_malformed_request_is_reported_as_an_invalid_request() {
        let error = NgsiLdError::from(AndromedaError::malformed_request("bad"));
        pretty_assertions::assert_eq!(error.error_type, NgsiLdErrorType::InvalidRequest);
        pretty_assertions::assert_eq!(error.detail, "bad");
    }

    #[test]
    fn an_internal_failure_never_reaches_the_client_in_detail() {
        let error = NgsiLdError::from(AndromedaError::internal("upstream timeout at db-7.internal"));
        pretty_assertions::assert_eq!(error.error_type, NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(error.detail, "Internal error");
    }
}
