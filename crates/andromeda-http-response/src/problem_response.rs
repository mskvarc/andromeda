//! Axum construction of a classified NGSI-LD problem response.

use andromeda_ngsi_ld_problem_details::problem_detail::ProblemDetail;
use axum::{
    Json,
    response::{IntoResponse, Response},
};

/// A local Axum response wrapper around the framework-independent clause 5.5.3 wire value.
#[derive(Debug)]
pub struct ProblemResponse {
    /// Invariant-bearing wire value.
    problem: ProblemDetail,
}

impl ProblemResponse {
    /// Wrap a fully classified and request-bound problem detail.
    pub const fn new(problem: ProblemDetail) -> ProblemResponse {
        ProblemResponse { problem }
    }

    /// Borrow the framework-independent wire value.
    pub const fn problem(&self) -> &ProblemDetail {
        &self.problem
    }
}

impl IntoResponse for ProblemResponse {
    fn into_response(self) -> Response {
        let status = self.problem.status_code();
        (status, Json(self.problem)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use crate::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use axum::{
        body::to_bytes,
        http::{StatusCode, Uri, header::CONTENT_TYPE},
        response::IntoResponse,
    };
    use serde_json::Value;

    async fn body(response: ProblemResponse) -> Result<(axum::response::Response, Value), serde_json::Error> {
        let response = response.into_response();
        let status = response.status();
        let headers = response.headers().clone();
        let value = match to_bytes(response.into_body(), usize::MAX).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) => return Err(serde_json::Error::io(std::io::Error::other(error))),
        };
        let mut rebuilt = axum::response::Response::new(axum::body::Body::empty());
        *rebuilt.status_mut() = status;
        *rebuilt.headers_mut() = headers;
        Ok((rebuilt, value))
    }

    #[tokio::test]
    async fn response_attaches_instance_and_uses_application_json() -> Result<(), serde_json::Error> {
        let response = ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, "Entity missing").with_instance(Uri::from_static("/ngsi-ld/v1/entities/x"));
        let (response, value) = body(response).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);
        pretty_assertions::assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        pretty_assertions::assert_eq!(value["instance"], "/ngsi-ld/v1/entities/x");
        pretty_assertions::assert_eq!(value["detail"], "Entity missing");
        Ok(())
    }

    #[tokio::test]
    async fn each_distributed_classification_uses_its_binding_status_and_fixed_payload_type() -> Result<(), serde_json::Error> {
        for (classification, expected_status, expected_type) in [
            (ProblemClassification::LoopDetected, StatusCode::LOOP_DETECTED, NgsiLdErrorType::InternalError),
            (
                ProblemClassification::GatewayTimeout,
                StatusCode::GATEWAY_TIMEOUT,
                NgsiLdErrorType::InternalError,
            ),
            (
                ProblemClassification::DistributedResourceNotFound,
                StatusCode::NOT_FOUND,
                NgsiLdErrorType::ResourceNotFound,
            ),
            (ProblemClassification::BadGateway, StatusCode::BAD_GATEWAY, NgsiLdErrorType::InternalError),
        ] {
            let response = ClassifiedReport::public_binding(classification, "forwarding failed").with_instance(Uri::from_static("/ngsi-ld/v1/entities/x"));
            let (response, value) = body(response).await?;
            pretty_assertions::assert_eq!(response.status(), expected_status);
            pretty_assertions::assert_eq!(value["status"], expected_status.as_u16());
            pretty_assertions::assert_eq!(value["type"], expected_type.type_iri().as_str());
        }
        Ok(())
    }
}
