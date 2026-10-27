//! Axum construction of a classified NGSI-LD problem response.

use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, NgsiLdWarningHeader};
use andromeda_ngsi_ld_problem_details::problem_detail::ProblemDetail;
use axum::{
    Json,
    response::{IntoResponse, Response},
};
use headers::{Connection, HeaderMapExt, RetryAfter};
use std::time::Duration;

/// Whether the connection carrying a response stays open after it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ConnectionAfterResponse {
    /// The connection stays as the protocol leaves it.
    #[default]
    Unannounced,
    /// The response announces `Connection: close`.
    Closes,
}

/// A local Axum response wrapper around the framework-independent clause 5.5.3 wire value.
#[derive(Debug)]
pub struct ProblemResponse {
    /// Invariant-bearing wire value.
    problem: ProblemDetail,
    /// Distributed-operation warnings emitted as `NGSILD-Warning` headers
    /// (clause 6.3.17): a forwarded read that found nothing still reports the
    /// sources it could not reach.
    warnings: Vec<NgsiLdWarning>,
    /// Optional delay before a recoverable request should be retried.
    retry_after: Option<Duration>,
    /// Whether the response announces `Connection: close`, as a refusal of a
    /// request whose content was not read does (RFC 9110 clause 10.1.1).
    connection: ConnectionAfterResponse,
}

impl ProblemResponse {
    /// Wrap a fully classified and request-bound problem detail.
    #[must_use]
    pub const fn new(problem: ProblemDetail) -> ProblemResponse {
        ProblemResponse {
            problem,
            warnings: Vec::new(),
            retry_after: None,
            connection: ConnectionAfterResponse::Unannounced,
        }
    }

    /// Attach the clause 6.3.17 warnings the failed operation gathered.
    #[must_use]
    pub fn with_warnings(mut self, warnings: Vec<NgsiLdWarning>) -> ProblemResponse {
        self.warnings = warnings;
        self
    }

    /// Borrow the framework-independent wire value.
    #[must_use]
    pub const fn problem(&self) -> &ProblemDetail {
        &self.problem
    }

    /// The warnings the response carries.
    #[must_use]
    pub fn warnings(&self) -> &[NgsiLdWarning] {
        &self.warnings
    }

    /// Attach the standard `Retry-After` delay form.
    #[must_use]
    pub const fn with_retry_after(mut self, retry_after: Duration) -> ProblemResponse {
        self.retry_after = Some(retry_after);
        self
    }

    /// Announce that the connection closes after this response.
    ///
    /// RFC 9110 clause 10.1.1: "A server that responds with a final status
    /// code before reading the entire request content SHOULD indicate whether
    /// it intends to close the connection". The header is HTTP/1.x only;
    /// RFC 9113 clause 8.2.2 forbids it on HTTP/2.
    #[must_use]
    pub const fn with_connection_close(mut self) -> ProblemResponse {
        self.connection = ConnectionAfterResponse::Closes;
        self
    }
}

impl IntoResponse for ProblemResponse {
    fn into_response(self) -> Response {
        let status = self.problem.status_code();
        let mut response = (status, Json(self.problem)).into_response();
        if !self.warnings.is_empty() {
            response.headers_mut().typed_insert(NgsiLdWarningHeader::new(self.warnings));
        }
        if let Some(retry_after) = self.retry_after {
            response.headers_mut().typed_insert(RetryAfter::delay(retry_after));
        }
        match self.connection {
            ConnectionAfterResponse::Unannounced => {}
            ConnectionAfterResponse::Closes => response.headers_mut().typed_insert(Connection::close()),
        }
        response
    }
}
#[cfg(test)]
mod tests {
    use crate::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use axum::{
        body::{Body, to_bytes},
        http::{
            StatusCode,
            Uri,
            header::{CONNECTION, CONTENT_TYPE},
        },
        response::{IntoResponse, Response},
    };
    use serde_json::Value;
    use std::{io::Error as IoError, time::Duration};

    async fn body(response: ProblemResponse) -> Result<(Response, Value), serde_json::Error> {
        let response = response.into_response();
        let status = response.status();
        let headers = response.headers().clone();
        let value = match to_bytes(response.into_body(), usize::MAX).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) => return Err(serde_json::Error::io(IoError::other(error))),
        };
        let mut rebuilt = Response::new(Body::empty());
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
    async fn recoverable_response_carries_retry_after_delay() -> Result<(), serde_json::Error> {
        let response = ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, "Snapshot promotion is still pending")
            .with_retry_after(Duration::from_secs(1))
            .with_instance(Uri::from_static("/ngsi-ld/v1/entities"));
        let (response, value) = body(response).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        pretty_assertions::assert_eq!(response.headers().get("retry-after").and_then(|value| value.to_str().ok()), Some("1"));
        pretty_assertions::assert_eq!(value["status"], 503);
        Ok(())
    }

    /// Clause 6.3.17: a distributed GET that ends in a 404 still reports the
    /// sources it could not reach, so the warnings ride on the problem response.
    #[tokio::test]
    async fn a_problem_response_carries_its_distributed_warnings_as_headers() -> Result<(), serde_json::Error> {
        let response = ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, "Entity missing")
            .with_instance(Uri::from_static("/ngsi-ld/v1/entities/x"))
            .with_warnings(vec![NgsiLdWarning::new(
                WarnCode::MiscellaneousWarning,
                "registration loop detected".to_string(),
            )]);
        let (response, _) = body(response).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);
        pretty_assertions::assert_eq!(
            response.headers().get("ngsild-warning").and_then(|value| value.to_str().ok()),
            Some("199 - \"registration loop detected\"")
        );
        Ok(())
    }

    /// RFC 9110 clause 10.1.1: a refusal answered before the content was read
    /// says the connection closes after it.
    #[tokio::test]
    async fn a_problem_response_can_announce_the_connection_close() -> Result<(), serde_json::Error> {
        let response = ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, "too large")
            .with_instance(Uri::from_static("/ngsi-ld/v1/entities"))
            .with_connection_close();
        let (response, value) = body(response).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(response.headers().get(CONNECTION).and_then(|value| value.to_str().ok()), Some("close"));
        pretty_assertions::assert_eq!(value["status"], 413);
        Ok(())
    }

    #[tokio::test]
    async fn a_problem_response_keeps_the_connection_unless_told_otherwise() -> Result<(), serde_json::Error> {
        let response = ClassifiedReport::public(NgsiLdErrorType::BadRequestData, "bad").with_instance(Uri::from_static("/ngsi-ld/v1/entities"));
        let (response, _) = body(response).await?;
        pretty_assertions::assert_eq!(response.headers().get(CONNECTION), None);
        Ok(())
    }

    #[tokio::test]
    async fn each_distributed_classification_uses_its_binding_status_and_fixed_payload_type() -> Result<(), serde_json::Error> {
        for (classification, expected_status, expected_type_iri) in [
            (
                ProblemClassification::LoopDetected,
                StatusCode::LOOP_DETECTED,
                "https://uri.etsi.org/ngsi-ld/errors/LoopDetected",
            ),
            (
                ProblemClassification::GatewayTimeout,
                StatusCode::GATEWAY_TIMEOUT,
                "https://uri.etsi.org/ngsi-ld/errors/GatewayTimeout",
            ),
            (
                ProblemClassification::DistributedResourceNotFound,
                StatusCode::NOT_FOUND,
                "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            ),
            (
                ProblemClassification::BadGateway,
                StatusCode::BAD_GATEWAY,
                "https://uri.etsi.org/ngsi-ld/errors/BadGateway",
            ),
        ] {
            let response = ClassifiedReport::public_binding(classification, "forwarding failed").with_instance(Uri::from_static("/ngsi-ld/v1/entities/x"));
            let (response, value) = body(response).await?;
            pretty_assertions::assert_eq!(response.status(), expected_status);
            pretty_assertions::assert_eq!(value["status"], expected_status.as_u16());
            pretty_assertions::assert_eq!(value["type"], expected_type_iri);
        }
        Ok(())
    }
}
