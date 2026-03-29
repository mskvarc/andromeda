use axum::{
    http::Uri,
    response::{IntoResponse, Response},
};
pub use ngsi_ld_error::{NgsiLdError, NgsiLdErrorType};

use crate::error::{AndromedaError, AndromedaErrorKind};

#[derive(Debug)]
pub struct ApiError {
    pub error: AndromedaError,
}

impl ApiError {
    pub fn from_error(error: AndromedaError, instance: Uri) -> Self {
        Self {
            error: error.with_instance(instance),
        }
    }
}

impl From<AndromedaError> for ApiError {
    fn from(error: AndromedaError) -> Self {
        Self { error }
    }
}

impl From<AndromedaError> for NgsiLdError {
    fn from(err: AndromedaError) -> Self {
        let instance = err.context().instance.clone().unwrap_or_else(|| Uri::from_static("/"));

        let error_type = match err.kind() {
            AndromedaErrorKind::InvalidRequest => NgsiLdErrorType::InvalidRequest,
            AndromedaErrorKind::BadRequestData => NgsiLdErrorType::BadRequestData,
            AndromedaErrorKind::NotFound => NgsiLdErrorType::ResourceNotFound,
            AndromedaErrorKind::AlreadyExists => NgsiLdErrorType::AlreadyExists,
            AndromedaErrorKind::Conflict => NgsiLdErrorType::Conflict,
            AndromedaErrorKind::LdContextNotAvailable => NgsiLdErrorType::LdContextNotAvailable,
            AndromedaErrorKind::NoMultiTenantSupport => NgsiLdErrorType::NoMultiTenantSupport,
            AndromedaErrorKind::NonexistentTenant => NgsiLdErrorType::NonexistentTenant,
            AndromedaErrorKind::OperationNotSupported => NgsiLdErrorType::OperationNotSupported,
            AndromedaErrorKind::Repository | AndromedaErrorKind::Json | AndromedaErrorKind::Io | AndromedaErrorKind::Config | AndromedaErrorKind::Internal => {
                NgsiLdErrorType::InternalError
            }
        };

        let detail = match err.kind() {
            AndromedaErrorKind::Repository | AndromedaErrorKind::Json | AndromedaErrorKind::Io | AndromedaErrorKind::Config | AndromedaErrorKind::Internal => {
                "Internal error".to_string()
            }
            _ => err.message().to_string(),
        };

        NgsiLdError::new(error_type, detail, instance)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let ngsi: NgsiLdError = self.error.into();
        ngsi.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{AndromedaError, AndromedaErrorKind};
    use axum::{body::to_bytes, http::Uri, response::IntoResponse};
    use serde_json::Value;

    #[test]
    fn error_kind_maps_to_ngsi_error_type() {
        let err = AndromedaError::invalid_request("bad");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.error_type, NgsiLdErrorType::InvalidRequest);
    }

    #[tokio::test]
    async fn response_includes_instance_and_status() {
        let err = AndromedaError::invalid_request("bad request").with_instance(Uri::from_static("/ngsi-ld/v1/entities"));
        let ngsi: NgsiLdError = err.into();
        let response = ngsi.into_response();

        let body = to_bytes(response.into_body(), usize::MAX).await.expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(json.get("instance").and_then(|v| v.as_str()), Some("/ngsi-ld/v1/entities"));
        assert_eq!(json.get("status").and_then(|v| v.as_u64()), Some(400));
    }

    #[test]
    fn internal_errors_are_sanitized() {
        let err = AndromedaError::new(AndromedaErrorKind::Internal, "secret");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.detail, "Internal error");
    }
}
