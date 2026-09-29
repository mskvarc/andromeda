//! Reading a request payload body within the broker's size limit.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.2 requires a "Request Entity too large"
//! (413) "when the HTTP input data stream provided by a client was too large
//! i.e. too many bytes". Every extractor that buffers a payload reads it here,
//! so each refuses an oversized body alike: as a problem detail that tells the
//! client how large a body the broker accepts, never as the framework's
//! plain-text 413.

use crate::{
    addressed_uri::addressed_request_uri,
    content_length_precondition::{DeclaredBodyLength, declared_body_length},
    request_body_limit::RequestBodyLimit,
};
use andromeda_http_response::{classified_report::ClassifiedReport, extraction_rejection::ExtractorRejection};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    body::Bytes,
    extract::{
        FromRequest,
        Request,
        rejection::{BytesRejection, FailedToBufferBody},
    },
    http::Uri,
};

/// What a client whose payload was refused as too large can do about it.
const SPLIT_ADVICE: &str = "send the content in smaller requests, a batch operation as several smaller batches";

/// A request payload body, read in full within the broker's size limit.
///
/// Applies the clause 6.3.4 `Content-Length` precondition first, so a request
/// that declares no length is the bare 411 the clause requires before anything
/// is read.
pub struct RequestPayload(pub Bytes);

impl<S> FromRequest<S> for RequestPayload
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(request: Request, state: &S) -> Result<RequestPayload, ExtractorRejection> {
        let instance = addressed_request_uri(&request);
        // clone: the precondition's refusal and the payload's each name the request.
        let declared = declared_body_length(request.headers()).map_err(|error| error.into_rejection(instance.clone()))?;
        buffer_payload(request, state, declared, instance).await.map(RequestPayload)
    }
}

/// Read the payload body of a request that declared `declared` bytes.
///
/// A declared length over the limit is refused before a byte of the body is
/// read. A body that runs past the limit while it is read -- one whose framing
/// the declared length does not bound -- is refused the same way.
///
/// # Errors
///
/// Returns a `RequestEntityTooLarge` problem (413) for a payload over the
/// limit, and `InvalidRequest` for a payload the connection failed to deliver.
pub async fn buffer_payload<S>(request: Request, state: &S, declared: DeclaredBodyLength, instance: Uri) -> Result<Bytes, ExtractorRejection>
where
    S: Send + Sync,
{
    let limit = request.extensions().get::<RequestBodyLimit>().copied();
    if let Some(limit) = limit
        && declares_more_than(declared, limit)
    {
        return Err(payload_too_large(Some(limit), instance));
    }
    Bytes::from_request(request, state).await.map_err(|rejection| {
        // `BytesRejection` and `FailedToBufferBody` are non-exhaustive, so the
        // limit is singled out and every other failure is a payload the
        // transport did not deliver.
        if matches!(&rejection, BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_))) {
            payload_too_large(limit, instance)
        } else {
            ExtractorRejection::invalid_request(format!("the request payload body could not be read: {}", rejection.body_text()), instance)
        }
    })
}

/// Whether the declared payload length exceeds the limit.
fn declares_more_than(declared: DeclaredBodyLength, limit: RequestBodyLimit) -> bool {
    u64::try_from(limit.bytes()).is_ok_and(|limit| declared.bytes().get() > limit)
}

/// The clause 6.3.2 "Request Entity too large" refusal, naming the limit when
/// the stack published one.
fn payload_too_large(limit: Option<RequestBodyLimit>, instance: Uri) -> ExtractorRejection {
    let detail = match limit {
        Some(limit) => format!(
            "the request payload body is larger than the {} bytes this broker accepts in one request; {SPLIT_ADVICE}",
            limit.bytes()
        ),
        None => format!("the request payload body is larger than this broker accepts in one request; {SPLIT_ADVICE}"),
    };
    ExtractorRejection::Classified(ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, detail).with_instance(instance))
}

#[cfg(test)]
mod tests {
    use crate::{request_body_limit::RequestBodyLimit, request_payload::RequestPayload};
    use axum::{
        Extension,
        Router,
        body::{Body, to_bytes},
        extract::DefaultBodyLimit,
        http::{Request, StatusCode, header::CONTENT_LENGTH},
        routing::post,
    };
    use futures::stream;
    use serde_json::Value;
    use std::convert::Infallible;
    use tower::ServiceExt;

    /// The limit every fixture router enforces.
    const LIMIT: usize = 16;

    /// The length of the payload the handler received.
    async fn payload_length(RequestPayload(body): RequestPayload) -> String {
        body.len().to_string()
    }

    /// A router enforcing [`LIMIT`], publishing it as the stack does when
    /// `published` is set.
    fn router(published: bool) -> Router {
        let router = Router::new().route("/ngsi-ld/v1/jsonldContexts", post(payload_length));
        let router = if published {
            router.layer(Extension(RequestBodyLimit::new(LIMIT)))
        } else {
            router
        };
        router.layer(DefaultBodyLimit::max(LIMIT))
    }

    /// The status and the parsed body a router answers `request` with.
    async fn answer(router: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = router.oneshot(request).await.expect("the router answers");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// A body `size` bytes long that declares its length.
    fn declared(size: usize) -> Request<Body> {
        Request::post("/ngsi-ld/v1/jsonldContexts")
            .header(CONTENT_LENGTH, size)
            .body(Body::from(vec![b'x'; size]))
            .expect("a valid request")
    }

    /// A body `size` bytes long whose declared length understates it, so only
    /// reading it can find it too large.
    fn understated(size: usize) -> Request<Body> {
        let chunks = stream::iter([Ok::<_, Infallible>(vec![b'x'; size])]);
        Request::post("/ngsi-ld/v1/jsonldContexts")
            .header(CONTENT_LENGTH, 1)
            .body(Body::from_stream(chunks))
            .expect("a valid request")
    }

    /// Clause 6.3.2: a payload over the limit is the 413 "Request Entity too
    /// large", carried as a problem detail naming the limit.
    #[tokio::test]
    async fn a_declared_length_over_the_limit_is_a_413_problem_naming_the_limit() {
        let (status, problem) = answer(router(true), declared(LIMIT + 1)).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
        pretty_assertions::assert_eq!(problem["title"], "Request entity too large");
        pretty_assertions::assert_eq!(problem["status"], 413);
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/jsonldContexts");
        let detail = problem["detail"].as_str().expect("a detail");
        assert!(detail.contains("16 bytes"), "{detail}");
    }

    /// A body whose framing runs past the limit is refused as it is read, with
    /// the same problem.
    #[tokio::test]
    async fn a_body_that_runs_past_the_limit_while_read_is_the_same_413_problem() {
        let (status, problem) = answer(router(true), understated(LIMIT + 1)).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
    }

    /// Without the published limit the refusal stands, only without the figure.
    #[tokio::test]
    async fn an_unpublished_limit_still_refuses_with_a_problem_detail() {
        let (status, problem) = answer(router(false), declared(LIMIT + 1)).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
        let detail = problem["detail"].as_str().expect("a detail");
        assert!(!detail.contains("bytes this broker"), "{detail}");
    }

    /// A payload at the limit is read in full.
    #[tokio::test]
    async fn a_payload_at_the_limit_is_read() {
        let response = router(true).oneshot(declared(LIMIT)).await.expect("the router answers");

        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
        pretty_assertions::assert_eq!(body.as_ref(), b"16".as_slice());
    }

    /// Clause 6.3.4: the length precondition comes first.
    #[tokio::test]
    async fn a_payload_declaring_no_length_is_the_bare_411() {
        let request = Request::post("/ngsi-ld/v1/jsonldContexts").body(Body::from("{}")).expect("a valid request");

        let (status, problem) = answer(router(true), request).await;

        pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED);
        pretty_assertions::assert_eq!(problem, Value::Null);
    }
}
