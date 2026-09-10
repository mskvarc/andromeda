//! The `Content-Length` precondition every body-taking operation checks.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.4: "For HTTP POST, PATCH and PUT HTTP
//! requests implementations shall check the following preconditions: [...]
//! Content-Length header shall include the length of the request payload
//! body." A request that fails it is refused before its body is read, and the
//! refusal is the one the clause names: "'Content-Length' HTTP header absence,
//! shall result in just a 411 HTTP status code (without any payload body)".
//!
//! Every extractor that consumes a request payload -- the JSON-LD body of a
//! single operation, the array of a batch operation, the document of a hosted
//! `@context` -- checks it here rather than each on its own, so no body-taking
//! route can be reached without the precondition having been applied.

use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{
    extract::{FromRequestParts, OriginalUri},
    http::{HeaderMap, StatusCode, Uri, header::CONTENT_LENGTH, request::Parts},
};
use std::num::NonZeroU64;
use thiserror::Error;

/// The payload length a request declared, once the clause 6.3.4 precondition
/// has admitted it.
///
/// The length is a [`NonZeroU64`] because a body-taking operation has no
/// reading of a zero-length payload: clause 5.5.4 raises `InvalidRequest` for a
/// payload body that "is not a valid JSON document", and an empty one is not.
/// Making the zero unrepresentable keeps that decision at the precondition
/// rather than at each body decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredBodyLength(NonZeroU64);

impl DeclaredBodyLength {
    /// The payload length the request declared, in bytes.
    #[must_use]
    pub const fn bytes(self) -> NonZeroU64 {
        self.0
    }
}

/// The ways a request fails the clause 6.3.4 `Content-Length` precondition.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ContentLengthPreconditionError {
    /// No `Content-Length` header at all. Clause 6.3.4 answers this one with a
    /// bare status, so the message is only ever read by a log.
    #[error("Content-Length is required")]
    Absent,
    /// A `Content-Length` that is not the decimal byte count RFC 9110 clause
    /// 8.6 defines.
    #[error("Content-Length shall be a non-negative decimal integer")]
    Malformed,
    /// A `Content-Length` of zero, which declares a payload body that clause
    /// 5.5.4 cannot read as a JSON document.
    #[error("Request body shall not be empty")]
    Empty,
}

impl ContentLengthPreconditionError {
    /// The refusal this failure renders as, bound to the request it refuses.
    ///
    /// Clause 6.3.4 splits the two: an absent header "shall result in just a
    /// 411 HTTP status code (without any payload body)", while every other
    /// unmet precondition keeps the clause's general rule that "an HTTP error
    /// response of type `InvalidRequest` shall be returned".
    #[must_use]
    pub fn into_rejection(self, instance: Uri) -> ExtractorRejection {
        match self {
            ContentLengthPreconditionError::Absent => ExtractorRejection::Empty(StatusCode::LENGTH_REQUIRED),
            ContentLengthPreconditionError::Malformed | ContentLengthPreconditionError::Empty => {
                ExtractorRejection::invalid_request(self.to_string(), instance)
            }
        }
    }
}

/// Apply the clause 6.3.4 `Content-Length` precondition to a request's headers.
///
/// The header is read untyped on purpose: [`headers::ContentLength`] reports an
/// absent header and an unparsable one alike, and clause 6.3.4 answers the
/// first with a `411` and the second with an `InvalidRequest`.
///
/// # Errors
///
/// Returns [`ContentLengthPreconditionError::Absent`] when the request declares
/// no `Content-Length`, [`ContentLengthPreconditionError::Malformed`] when the
/// value it declares is not a decimal byte count, and
/// [`ContentLengthPreconditionError::Empty`] when it declares a zero-length
/// payload body.
pub fn declared_body_length(headers: &HeaderMap) -> Result<DeclaredBodyLength, ContentLengthPreconditionError> {
    let declared = headers.get(CONTENT_LENGTH).ok_or(ContentLengthPreconditionError::Absent)?;
    let declared = declared
        .to_str()
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(ContentLengthPreconditionError::Malformed)?;
    NonZeroU64::new(declared).map(DeclaredBodyLength).ok_or(ContentLengthPreconditionError::Empty)
}

impl<S> FromRequestParts<S> for DeclaredBodyLength
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<DeclaredBodyLength, Self::Rejection> {
        // OriginalUri is infallible, so unwrap via match. It names the request
        // as the client addressed it, which is the `instance` of any problem
        // the precondition reports.
        let OriginalUri(uri) = match OriginalUri::from_request_parts(parts, state).await {
            Ok(original) => original,
            Err(never) => match never {},
        };
        declared_body_length(&parts.headers).map_err(|error| error.into_rejection(uri))
    }
}

#[cfg(test)]
mod tests {
    use crate::content_length_precondition::{ContentLengthPreconditionError, DeclaredBodyLength, declared_body_length};
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use axum::{
        body::{Body, to_bytes},
        extract::FromRequestParts,
        http::{HeaderMap, HeaderValue, Request, StatusCode, header::CONTENT_LENGTH},
        response::IntoResponse,
    };
    use std::num::NonZeroU64;

    /// Headers declaring one raw `Content-Length` value.
    fn headers_declaring(length: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static(length));
        headers
    }

    /// Clause 6.3.4: "Content-Length header shall include the length of the
    /// request payload body."
    #[test]
    fn a_declared_length_is_admitted_as_the_payload_length() {
        pretty_assertions::assert_eq!(
            declared_body_length(&headers_declaring("42")),
            Ok(DeclaredBodyLength(NonZeroU64::new(42).expect("42 is not zero")))
        );
    }

    /// Clause 6.3.4: an absent header is the one precondition failure answered
    /// with "just a 411 HTTP status code (without any payload body)".
    #[test]
    fn an_absent_content_length_is_the_411_precondition() {
        pretty_assertions::assert_eq!(declared_body_length(&HeaderMap::new()), Err(ContentLengthPreconditionError::Absent));
    }

    /// A `Content-Length` that is not a decimal byte count declares no length,
    /// and is refused as `InvalidRequest` rather than as an absent header.
    #[test]
    fn a_malformed_content_length_is_not_an_absent_one() {
        for value in ["not-a-number", "-1", "1.5", ""] {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_LENGTH, HeaderValue::from_str(value).expect("visible ASCII header value"));
            pretty_assertions::assert_eq!(
                declared_body_length(&headers),
                Err(ContentLengthPreconditionError::Malformed),
                "Content-Length: {value}"
            );
        }
    }

    /// Clause 5.5.4: an empty payload body is not a valid JSON document, so a
    /// declared zero is refused rather than passed to a body decoder.
    #[test]
    fn a_zero_content_length_declares_an_empty_body() {
        pretty_assertions::assert_eq!(declared_body_length(&headers_declaring("0")), Err(ContentLengthPreconditionError::Empty));
    }

    /// Clause 6.3.4: the `411` carries no payload body; the other two failures
    /// keep the clause's `InvalidRequest`, which is a `400` with a problem
    /// detail.
    #[tokio::test]
    async fn each_failure_renders_the_status_the_clause_names() {
        for (error, expected_status, expects_body) in [
            (ContentLengthPreconditionError::Absent, StatusCode::LENGTH_REQUIRED, false),
            (ContentLengthPreconditionError::Malformed, StatusCode::BAD_REQUEST, true),
            (ContentLengthPreconditionError::Empty, StatusCode::BAD_REQUEST, true),
        ] {
            let response = error
                .into_rejection("/ngsi-ld/v1/entities".parse().expect("a valid request URI"))
                .into_response();
            pretty_assertions::assert_eq!(response.status(), expected_status, "{error}");
            let body = to_bytes(response.into_body(), usize::MAX).await.expect("a rendered body");
            pretty_assertions::assert_eq!(!body.is_empty(), expects_body, "{error}: {body:?}");
        }
    }

    /// The extractor form applies the same precondition to a request's parts.
    #[tokio::test]
    async fn the_extractor_refuses_a_request_that_declares_no_length() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/jsonldContexts")
            .body(Body::empty())
            .expect("a valid request");
        let (mut parts, _) = request.into_parts();

        let rejection = DeclaredBodyLength::from_request_parts(&mut parts, &())
            .await
            .expect_err("a request declaring no length is refused");

        match rejection {
            ExtractorRejection::Empty(status) => pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED),
            ExtractorRejection::NotAcceptable(_) | ExtractorRejection::Classified(_) => panic!("clause 6.3.4 requires a bare 411"),
            ExtractorRejection::InvalidRequest { detail, .. } | ExtractorRejection::BadRequestData { detail, .. } => {
                panic!("clause 6.3.4 requires a bare 411, not a problem detail: {detail}")
            }
        }
    }

    /// A request that declares a length reaches its body decoder.
    #[tokio::test]
    async fn the_extractor_admits_a_request_that_declares_a_length() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/jsonldContexts")
            .header(CONTENT_LENGTH, "17")
            .body(Body::empty())
            .expect("a valid request");
        let (mut parts, _) = request.into_parts();

        let length = DeclaredBodyLength::from_request_parts(&mut parts, &())
            .await
            .expect("a declared length is admitted");

        pretty_assertions::assert_eq!(length.bytes().get(), 17);
    }
}
