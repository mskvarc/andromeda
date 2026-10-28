//! Reading a request payload body within the broker's size limit.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.2 requires a "Request Entity too large"
//! (413) "when the HTTP input data stream provided by a client was too large
//! i.e. too many bytes". Every extractor that buffers a payload reads it here,
//! so each refuses an oversized body alike: as a problem detail that tells the
//! client how large a body the broker accepts, never as the framework's
//! plain-text 413.
//!
//! Over HTTP/1.x the refusal leaves content unread on the connection, and the
//! only way to stop the upload is to close the connection. The problem detail
//! says so with `Connection: close`, as RFC 9110 clause 10.1.1 asks of a server
//! that "responds with a final status code before reading the entire request
//! content"; the listener then closes the connection in stages, so the client
//! reads the refusal before the connection is reset. HTTP/2 and HTTP/3 stop the
//! upload with a stream reset instead and forbid the header.
//!
//! [`RequestPayload`] reads a JSON document an operation parses itself, so it
//! also holds the payload to the JSON value limit the stack publishes, and its
//! text to the request text admission every other body passes, before the
//! operation builds a tree from it.

use crate::{
    addressed_uri::addressed_request_uri,
    body_admission_refusal::BodyAdmissionRefusal,
    content_kind::request_content_kind,
    content_length_precondition::{DeclaredBodyLength, declared_body_length},
    json_value_budget::refuse_values_past_budget,
    request_body_limit::RequestBodyLimit,
    request_json_value_limit::RequestJsonValueLimit,
};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    extraction_rejection::ExtractorRejection,
    problem_response::ProblemResponse,
    request_refusal::RequestRefusal,
};
use andromeda_ngsi_ld_number::json_value::text_admission::{JsonTextRefusal, admit_json_text};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    body::Bytes,
    extract::{
        FromRequest,
        Request,
        rejection::{BytesRejection, FailedToBufferBody},
    },
    http::{StatusCode, Uri, Version},
};
use thiserror::Error;

/// What a client whose payload was refused as too large can do about it.
pub(crate) const SPLIT_ADVICE: &str = "send the content in smaller requests, a batch operation as several smaller batches";

/// A JSON request payload body, read in full within the broker's size limit
/// and counted within its JSON value limit.
///
/// Applies the clause 6.3.4 `Content-Length` precondition first, so a request
/// that declares no length is the bare 411 the clause requires before anything
/// is read. A payload that is no JSON text passes the count and the text
/// admission, and the parse that follows refuses it in its operation's own
/// words.
pub struct RequestPayload(pub Bytes);

impl<S> FromRequest<S> for RequestPayload
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(request: Request, state: &S) -> Result<RequestPayload, ExtractorRejection> {
        let instance = addressed_request_uri(&request);
        // clone: the precondition's refusal and the payload's each name the request.
        let declared = declared_body_length(request.headers(), request.extensions()).map_err(|error| error.into_rejection(instance.clone()))?;
        // Clause 6.3.4: the `Content-Type` is `application/json` or
        // `application/ld+json` (`application/merge-patch+json` for PATCH
        // alone); anything else is "just a 415 HTTP status code".
        if request_content_kind(request.headers(), request.method()).is_none() {
            return Err(ExtractorRejection::Empty(StatusCode::UNSUPPORTED_MEDIA_TYPE));
        }
        let value_limit = request.extensions().get::<RequestJsonValueLimit>().copied();
        // clone: a counted payload over its limit names the request too.
        let body = buffer_payload(request, state, declared, instance.clone()).await?;
        // Counted before the operation builds a tree, so a payload of too
        // many values costs its bytes and nothing more.
        // clone: a refused text names the request too.
        refuse_values_past_budget(&body, value_limit).map_err(|refusal| ExtractorRejection::Classified(refusal.report().with_instance(instance.clone())))?;
        // A text that is no JSON is left to the operation's parse, which words
        // that refusal itself.
        match admit_json_text(&body) {
            Ok(()) | Err(JsonTextRefusal::Syntax(_)) => Ok(RequestPayload(body)),
            Err(
                refused @ (JsonTextRefusal::OutOfRange(_)
                | JsonTextRefusal::Unholdable { .. }
                | JsonTextRefusal::ReservedMemberName { .. }
                | JsonTextRefusal::DuplicateMember { .. }),
            ) => Err(BodyAdmissionRefusal::Inadmissible(refused).into_rejection(instance)),
        }
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
    let version = request.version();
    if let Some(limit) = limit
        && declares_more_than(declared, limit)
    {
        return Err(payload_too_large(Some(limit), instance, version));
    }
    Bytes::from_request(request, state).await.map_err(|rejection| {
        // `BytesRejection` and `FailedToBufferBody` are non-exhaustive, so the
        // limit is singled out and every other failure is a payload the
        // transport did not deliver.
        if matches!(&rejection, BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_))) {
            payload_too_large(limit, instance, version)
        } else {
            UnreadablePayload(rejection).into_rejection(instance)
        }
    })
}

/// Whether the declared payload length exceeds the limit.
fn declares_more_than(declared: DeclaredBodyLength, limit: RequestBodyLimit) -> bool {
    u64::try_from(limit.bytes()).is_ok_and(|limit| declared.bytes().get() > limit)
}

/// The clause 6.3.2 "Request Entity too large" refusal of a request received
/// over `version`, naming the limit when the stack published one.
fn payload_too_large(limit: Option<RequestBodyLimit>, instance: Uri, version: Version) -> ExtractorRejection {
    let detail = match limit {
        Some(limit) => PayloadTooLarge::Limited { bytes: limit.bytes() },
        None => PayloadTooLarge::Unpublished,
    };
    let refusal = ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, detail.to_string()).with_instance(instance);
    ExtractorRejection::Classified(closing_http1_connection(refusal, version))
}

/// A payload body past the size the broker accepts in one request.
#[derive(Debug, Error)]
enum PayloadTooLarge {
    /// The stack published its limit.
    #[error("the request payload body is larger than the {bytes} bytes this broker accepts in one request; {advice}", advice = SPLIT_ADVICE)]
    Limited {
        /// The published limit in bytes.
        bytes: usize,
    },
    /// The stack published no limit.
    #[error("the request payload body is larger than this broker accepts in one request; {advice}", advice = SPLIT_ADVICE)]
    Unpublished,
}

/// A payload body the connection failed to deliver.
#[derive(Debug, Error)]
#[error("the request payload body could not be read: {0}")]
struct UnreadablePayload(#[source] BytesRejection);

impl RequestRefusal for UnreadablePayload {
    fn error_type(&self) -> NgsiLdErrorType {
        NgsiLdErrorType::InvalidRequest
    }
}

/// Announce the close of an HTTP/1.x connection whose request content the
/// refusal leaves unread; later versions stop the upload with a stream reset.
fn closing_http1_connection(refusal: ProblemResponse, version: Version) -> ProblemResponse {
    if version < Version::HTTP_2 {
        refusal.with_connection_close()
    } else {
        refusal
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{request_body_limit::RequestBodyLimit, request_json_value_limit::RequestJsonValueLimit, request_payload::RequestPayload};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use axum::{
        Extension,
        Router,
        body::{Body, to_bytes},
        extract::DefaultBodyLimit,
        http::{
            Request,
            StatusCode,
            Version,
            header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE},
        },
        routing::post,
    };
    use futures::stream;
    use serde_json::Value;
    use std::{convert::Infallible, num::NonZeroUsize};
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

    /// The status a router answers `request` with, and the `Connection` header
    /// it announces.
    async fn connection_announced(router: Router, request: Request<Body>) -> (StatusCode, Option<String>) {
        let response = router.oneshot(request).await.expect("the router answers");
        let connection = response.headers().get(CONNECTION).and_then(|value| value.to_str().ok()).map(str::to_owned);
        (response.status(), connection)
    }

    /// A body `size` bytes long that declares its length.
    fn declared(size: usize) -> Request<Body> {
        Request::post("/ngsi-ld/v1/jsonldContexts")
            .header(CONTENT_LENGTH, size)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(vec![b'x'; size]))
            .expect("a valid request")
    }

    /// A body `size` bytes long whose declared length understates it, so only
    /// reading it can find it too large.
    fn understated(size: usize) -> Request<Body> {
        let chunks = stream::iter([Ok::<_, Infallible>(vec![b'x'; size])]);
        Request::post("/ngsi-ld/v1/jsonldContexts")
            .header(CONTENT_LENGTH, 1)
            .header(CONTENT_TYPE, "application/json")
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

    /// A payload the operation parses itself passes the text admission every
    /// body passes: a string holding U+0000 is `BadRequestData` at its
    /// location, while a text that is no JSON is left to the operation.
    #[tokio::test]
    async fn a_payload_holding_u0000_is_bad_request_data_naming_where_it_stands() {
        let body = r#"{"a":"\u0000"}"#;
        let request = Request::post("/ngsi-ld/v1/jsonldContexts")
            .header(CONTENT_LENGTH, body.len())
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("a valid request");
        let (status, problem) = answer(router(true), request).await;

        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        let detail = problem["detail"].as_str().expect("a detail");
        assert!(detail.contains("/a") && detail.contains("U+0000"), "{detail}");
    }

    /// RFC 9110 clause 10.1.1: refusing an HTTP/1.1 upload before reading it
    /// announces that the connection closes after the refusal.
    #[tokio::test]
    async fn a_declared_length_over_the_limit_on_http1_announces_the_close() {
        let (status, connection) = connection_announced(router(true), declared(LIMIT + 1)).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(connection.as_deref(), Some("close"));
    }

    /// A body refused once it runs past the limit leaves the rest unread, so
    /// its refusal announces the close too.
    #[tokio::test]
    async fn a_body_that_runs_past_the_limit_on_http1_announces_the_close() {
        let (status, connection) = connection_announced(router(true), understated(LIMIT + 1)).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(connection.as_deref(), Some("close"));
    }

    /// RFC 9113 clause 8.2.2 forbids `Connection` on HTTP/2, which stops an
    /// upload with a stream reset instead.
    #[tokio::test]
    async fn a_refusal_over_http2_announces_no_close() {
        let mut request = declared(LIMIT + 1);
        *request.version_mut() = Version::HTTP_2;

        let (status, connection) = connection_announced(router(true), request).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(connection, None);
    }

    /// A payload read in full leaves the connection as it was.
    #[tokio::test]
    async fn a_payload_at_the_limit_announces_no_close() {
        let (status, connection) = connection_announced(router(true), declared(LIMIT)).await;

        pretty_assertions::assert_eq!(status, StatusCode::OK);
        pretty_assertions::assert_eq!(connection, None);
    }

    /// Clause 6.3.4: the length precondition comes first.
    #[tokio::test]
    async fn a_payload_declaring_no_length_is_the_bare_411() {
        let request = Request::post("/ngsi-ld/v1/jsonldContexts")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .expect("a valid request");

        let (status, problem) = answer(router(true), request).await;

        pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED);
        pretty_assertions::assert_eq!(problem, Value::Null);
    }

    /// Clause 6.3.4: `application/merge-patch+json` on a POST, and a missing
    /// `Content-Type`, are the bare 415.
    #[tokio::test]
    async fn a_post_payload_without_a_json_media_type_is_the_bare_415() {
        for content_type in [Some("application/merge-patch+json"), Some("text/plain"), None] {
            let mut builder = Request::post("/ngsi-ld/v1/jsonldContexts").header(CONTENT_LENGTH, 2);
            if let Some(content_type) = content_type {
                builder = builder.header(CONTENT_TYPE, content_type);
            }
            let request = builder.body(Body::from("{}")).expect("a valid request");

            let (status, problem) = answer(router(true), request).await;

            pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{content_type:?}");
            pretty_assertions::assert_eq!(problem, Value::Null);
        }
    }

    /// Clause 6.3.2: a JSON payload holding more values than the published
    /// limit is the 413 "Request Entity too large", counted before the
    /// operation parses it; one within the limit, and a payload that is no
    /// JSON text, are read.
    #[tokio::test]
    async fn a_payload_past_the_json_value_limit_is_a_413_problem() {
        let three = RequestJsonValueLimit::new(JsonValueLimit::new(NonZeroUsize::new(3).expect("three is not zero")));
        let counted = || {
            Router::new()
                .route("/ngsi-ld/v1/entityOperations/delete", post(payload_length))
                .layer(Extension(three))
        };
        let request = |body: &'static str| {
            Request::post("/ngsi-ld/v1/entityOperations/delete")
                .header(CONTENT_LENGTH, body.len())
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .expect("a valid request")
        };

        let (status, problem) = answer(counted(), request(r#"["urn:a:1","urn:a:2","urn:a:3"]"#)).await;
        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
        assert!(problem["detail"].as_str().expect("a detail").contains("3 JSON values"), "{problem}");

        pretty_assertions::assert_eq!(answer(counted(), request(r#"["urn:a:1","urn:a:2"]"#)).await.0, StatusCode::OK);
        pretty_assertions::assert_eq!(answer(counted(), request("not json at all")).await.0, StatusCode::OK);
    }
}
