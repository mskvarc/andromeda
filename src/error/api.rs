use axum::{
    http::Uri,
    response::{IntoResponse, Response},
};

use crate::error::{
    AndromedaError,
    ngsi_ld::{NgsiLdError, ToProblem},
};

#[derive(Debug)]
pub struct ApiError {
    pub error: AndromedaError,
}

impl ApiError {
    pub fn from_error(error: AndromedaError, instance: Uri) -> ApiError {
        ApiError {
            error: error.with_instance(instance),
        }
    }
}

impl From<AndromedaError> for ApiError {
    fn from(error: AndromedaError) -> ApiError {
        ApiError { error }
    }
}

impl From<AndromedaError> for NgsiLdError {
    fn from(err: AndromedaError) -> NgsiLdError {
        let error_type = err.kind().to_ngsi_ld_error_type();
        let instance = err.context().instance.clone().unwrap_or_else(|| Uri::from_static("/"));

        let detail = if err.kind().is_internal() {
            "Internal error".to_string()
        } else {
            err.message().to_string()
        };

        NgsiLdError::new(error_type, detail, instance)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.error.kind().is_internal() {
            tracing::error!(
                error = %self.error,
                kind = ?self.error.kind(),
                "Internal error (sanitized before sending to client)"
            );
        }

        let (status, problem) = self.error.to_problem();
        (status, axum::Json(problem)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{
        core::{AndromedaError, AndromedaErrorKind},
        ngsi_ld::{NgsiLdErrorType, ToProblem},
    };
    use axum::http::Uri;
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
        let (status, problem) = err.to_problem();
        assert_eq!(status.as_u16(), 400);

        let json: Value = serde_json::to_value(&problem).expect("serialize problem");
        assert_eq!(json.get("instance").and_then(|v| v.as_str()), Some("/ngsi-ld/v1/entities"));
        assert_eq!(json.get("status").and_then(|v| v.as_u64()), Some(400));
    }

    #[test]
    fn internal_errors_are_sanitized() {
        let err = AndromedaError::new(AndromedaErrorKind::Internal, "secret");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.detail, "Internal error");
    }

    #[test]
    fn external_dependency_is_sanitized() {
        let err = AndromedaError::external_dependency("upstream timeout");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.error_type, NgsiLdErrorType::InternalError);
        assert_eq!(ngsi.detail, "Internal error");
    }

    #[test]
    fn repository_error_is_sanitized() {
        let err = AndromedaError::repository("connection refused");
        let ngsi: NgsiLdError = err.into();
        assert_eq!(ngsi.error_type, NgsiLdErrorType::InternalError);
        assert_eq!(ngsi.detail, "Internal error");
    }
}
