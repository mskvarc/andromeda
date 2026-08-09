//! Transitional classification of broker-owned failures at their current HTTP call sites.
//!
//! Wire construction and Axum response encoding are owned by `andromeda-http-response`; this module only maps the aggregate errors still used by bindings that have not yet been extracted.

use crate::{
    error::{AndromedaError, ErrorKind},
    http::error::category,
};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
use axum::http::Uri;

impl From<AndromedaError> for ProblemResponse {
    fn from(error: AndromedaError) -> ProblemResponse {
        let instance = error.instance().cloned().unwrap_or_else(|| Uri::from_static("/"));
        let report = if error.is_internal() {
            ClassifiedReport::internal(&error)
        } else if matches!(error.kind(), ErrorKind::ContextSourceGateway(_)) {
            ClassifiedReport::public_binding(ProblemClassification::BadGateway, error.to_string())
        } else {
            ClassifiedReport::public(category::of(error.kind()), error.to_string())
        };
        report.with_instance(instance)
    }
}

#[cfg(test)]
mod tests {
    use crate::error::AndromedaError;
    use andromeda_http_response::problem_response::ProblemResponse;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        body::to_bytes,
        http::{StatusCode, Uri},
        response::IntoResponse,
    };
    use serde_json::Value;

    #[tokio::test]
    async fn a_broker_failure_uses_the_shared_wire_and_keeps_its_instance() -> Result<(), Box<dyn std::error::Error>> {
        let error = AndromedaError::malformed_request("bad request").with_instance(&Uri::from_static("/ngsi-ld/v1/subscriptions"));
        let response = ProblemResponse::from(error).into_response();
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let problem: Value = serde_json::from_slice(&body)?;
        pretty_assertions::assert_eq!(problem["type"], NgsiLdErrorType::InvalidRequest.type_iri().into_inner());
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/subscriptions");
        Ok(())
    }

    #[test]
    fn an_internal_failure_is_sanitized_by_the_shared_disclosure_boundary() {
        let response = ProblemResponse::from(AndromedaError::internal("upstream timeout at db-7.internal"));
        pretty_assertions::assert_eq!(response.problem().error_type(), NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(response.problem().detail(), "Internal error");
    }

    #[test]
    fn a_context_source_gateway_failure_has_a_typed_502_binding() {
        let response = ProblemResponse::from(AndromedaError::context_source_gateway("mandatory source failed"));
        pretty_assertions::assert_eq!(response.problem().status_code(), StatusCode::BAD_GATEWAY);
        pretty_assertions::assert_eq!(response.problem().error_type(), NgsiLdErrorType::InternalError);
    }
}
