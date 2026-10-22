//! Extractor rejection types for NGSI-LD API error responses.
//!
//! Maps extractor-level failures to the appropriate NGSI-LD error types
//! (`InvalidRequest`, `BadRequestData`) as required by the specification.

use crate::{not_acceptable::AvailableRepresentations, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::problem_detail::ProblemDetail;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Rejection type for NGSI-LD API extractors.
///
/// Converts extraction failures into spec-compliant NGSI-LD error responses
/// with the correct HTTP status code and problem detail body.
#[derive(Debug)]
pub enum ExtractorRejection {
    /// A status code with no payload body, which clause 6.3.4 requires of the
    /// `411` an absent `Content-Length` raises and of the `415` an unsupported
    /// `Content-Type` raises: each results in "just a ... HTTP status code
    /// (without any payload body)".
    Empty(StatusCode),
    /// Clause 6.3.4: an `Accept` header naming no representation the endpoint
    /// can answer with results in "a 406 HTTP status code and the body of the
    /// message shall contain the list of the available representations of the
    /// resources".
    NotAcceptable(AvailableRepresentations),
    /// A problem response already classified by the typed refusal or the
    /// policy-specific extractor that produced it, bound to its request.
    Classified(ProblemResponse),
}

impl ExtractorRejection {
    /// The problem a classified rejection answers with; `None` for a bare
    /// status or the `406` representation list.
    #[must_use]
    pub const fn problem(&self) -> Option<&ProblemDetail> {
        match self {
            ExtractorRejection::Classified(response) => Some(response.problem()),
            ExtractorRejection::Empty(_) | ExtractorRejection::NotAcceptable(_) => None,
        }
    }
}

impl IntoResponse for ExtractorRejection {
    fn into_response(self) -> Response {
        match self {
            ExtractorRejection::Empty(status) => status.into_response(),
            ExtractorRejection::NotAcceptable(representations) => representations.into_response(),
            ExtractorRejection::Classified(response) => response.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{extraction_rejection::ExtractorRejection, not_acceptable::AvailableRepresentations};
    use andromeda_http_headers::media_type::GeoJsonSupport;
    use axum::{
        body::to_bytes,
        http::StatusCode,
        response::{IntoResponse, Response},
    };
    use serde_json::{Value, json};

    /// The status and the exact bytes a rejection renders as.
    async fn rendered(rejection: ExtractorRejection) -> (StatusCode, Vec<u8>) {
        let response: Response = rejection.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap_or_default();
        (status, bytes.to_vec())
    }

    /// Clause 6.3.4: an absent `Content-Length` results in "just a 411 HTTP
    /// status code (without any payload body)", and an unsupported
    /// `Content-Type` in "just a 415 HTTP status code (without any payload
    /// body)".
    #[tokio::test]
    async fn the_bodyless_preconditions_render_a_status_and_nothing_else() {
        for status in [StatusCode::LENGTH_REQUIRED, StatusCode::UNSUPPORTED_MEDIA_TYPE] {
            let (rendered_status, body) = rendered(ExtractorRejection::Empty(status)).await;
            pretty_assertions::assert_eq!(rendered_status, status);
            assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on {status}: {body:?}");
        }
    }

    /// Clause 6.3.4: the `406` alone carries a body, "the list of the available
    /// representations of the resources".
    #[tokio::test]
    async fn the_not_acceptable_rejection_renders_the_endpoints_representations() -> Result<(), serde_json::Error> {
        let (status, body) = rendered(ExtractorRejection::NotAcceptable(AvailableRepresentations::of(GeoJsonSupport::Disallowed))).await;

        pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
        let value: Value = serde_json::from_slice(&body)?;
        pretty_assertions::assert_eq!(value, json!({"availableRepresentations": ["application/json", "application/ld+json"]}));
        Ok(())
    }
}
