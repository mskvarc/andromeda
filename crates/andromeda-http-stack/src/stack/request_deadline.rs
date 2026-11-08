//! The wall-clock bound on one request.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.6 has an operation that cannot complete,
//! a database timeout included, raise `InternalError`, which Table 6.3.2-1
//! answers with 500. A request still in flight past its deadline is answered
//! with that problem document, and its handler future is dropped with whatever
//! store connections it held.
//!
//! A request whose client has not finished sending its body by then is the
//! client's failure, not the broker's: RFC 9110 clause 15.5.9 answers it 408
//! and has the server close the connection. The body is left unread, so the
//! TCP listener announces that close over HTTP/1.x as it does for every
//! response to unread content; HTTP/2 and HTTP/3 end only the stream.

use crate::stack::body_arrival::{BodyArrival, HeldUpBy};
use andromeda_http_extraction::addressed_uri::addressed_request_uri;
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Duration;
use thiserror::Error;
use tokio::time::timeout;

/// The longest one request may run before it is answered as failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RequestDeadline(Duration);

impl RequestDeadline {
    /// A deadline of `limit` from the moment the request reaches the layer.
    #[must_use]
    pub(super) const fn new(limit: Duration) -> RequestDeadline {
        RequestDeadline(limit)
    }
}

/// A request ran past its deadline without being answered.
#[derive(Clone, Copy, Debug, Error)]
#[error("the request did not complete within {} ms", .0.as_millis())]
struct DeadlinePassed(Duration);

/// A request body was still arriving when the deadline passed.
#[derive(Clone, Copy, Debug, Error)]
#[error("the request body did not arrive within {} ms", .0.as_millis())]
struct BodyLate(Duration);

/// Run the rest of the stack, answering once the deadline passes first: 408
/// `RequestTimeout` while the stack waits on the client for more of the body,
/// 500 `InternalError` otherwise.
pub(super) async fn answer_within_deadline(State(deadline): State<RequestDeadline>, request: Request, next: Next) -> Response {
    let instance = addressed_request_uri(&request);
    let (request, arrival) = BodyArrival::watch(request);
    match timeout(deadline.0, next.run(request)).await {
        Ok(response) => response,
        Err(_elapsed) => match arrival.held_up_by() {
            HeldUpBy::Broker => ClassifiedReport::public(NgsiLdErrorType::InternalError, DeadlinePassed(deadline.0).to_string())
                .with_instance(instance)
                .into_response(),
            HeldUpBy::Client => ClassifiedReport::public(NgsiLdErrorType::RequestTimeout, BodyLate(deadline.0).to_string())
                .with_instance(instance)
                .into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::stack::request_deadline::{RequestDeadline, answer_within_deadline};
    use axum::{
        Router,
        body::{Body, Bytes, to_bytes},
        extract::Request,
        http::{Error as RequestBuildError, StatusCode, Version, header::CONTENT_TYPE},
        middleware::from_fn_with_state,
        routing::{get, post},
    };
    use futures::stream::{StreamExt, iter, pending as pending_stream};
    use serde_json::Value;
    use std::{
        convert::Infallible,
        error::Error,
        future::pending,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    use tower::ServiceExt;

    fn bounded(router: Router, limit: Duration) -> Router {
        router.layer(from_fn_with_state(RequestDeadline::new(limit), answer_within_deadline))
    }

    /// Raises its flag once dropped, standing in for the rows and connections a
    /// handler holds.
    struct Held(Arc<AtomicBool>);

    impl Drop for Held {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn a_request_outliving_its_deadline_has_released_what_its_handler_held_when_answered() -> Result<(), Box<dyn Error>> {
        let released = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&released);
        let handler = move || {
            let held = Held(Arc::clone(&flag));
            async move {
                let _held = held;
                pending::<StatusCode>().await
            }
        };
        let router = bounded(Router::new().route("/ngsi-ld/v1/entities", get(handler)), Duration::from_millis(20));
        let response = router.oneshot(Request::get("/ngsi-ld/v1/entities?georel=near").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(released.load(Ordering::SeqCst), "the handler still holds its state after the deadline answered");
        Ok(())
    }

    #[tokio::test]
    async fn a_request_outliving_its_deadline_is_answered_with_an_internal_error_problem() -> Result<(), Box<dyn Error>> {
        let router = bounded(
            Router::new().route("/ngsi-ld/v1/entities", get(pending::<StatusCode>)),
            Duration::from_millis(20),
        );
        let response = router.oneshot(Request::get("/ngsi-ld/v1/entities?type=Room").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        pretty_assertions::assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let problem: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/InternalError");
        pretty_assertions::assert_eq!(problem["status"], 500);
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/entities?type=Room");
        pretty_assertions::assert_eq!(problem["detail"], "the request did not complete within 20 ms");
        Ok(())
    }

    /// The client sends the start of the body and then nothing more; the
    /// handler reads the body before it answers, as every payload handler does.
    fn stalled_body_request(version: Version) -> Result<Request, RequestBuildError> {
        let chunks = iter([Ok::<_, Infallible>(Bytes::from_static(b"{"))]).chain(pending_stream());
        Request::post("/ngsi-ld/v1/entities").version(version).body(Body::from_stream(chunks))
    }

    fn reading_router(limit: Duration) -> Router {
        bounded(
            Router::new().route("/ngsi-ld/v1/entities", post(|body: Bytes| async move { (StatusCode::CREATED, body) })),
            limit,
        )
    }

    #[tokio::test]
    async fn a_body_still_arriving_at_the_deadline_is_answered_with_a_request_timeout_problem() -> Result<(), Box<dyn Error>> {
        let response = reading_router(Duration::from_millis(20))
            .oneshot(stalled_body_request(Version::HTTP_11)?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        let problem: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestTimeout");
        pretty_assertions::assert_eq!(problem["title"], "Request timeout");
        pretty_assertions::assert_eq!(problem["status"], 408);
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/entities");
        pretty_assertions::assert_eq!(problem["detail"], "the request body did not arrive within 20 ms");
        Ok(())
    }

    #[tokio::test]
    async fn a_body_read_whole_before_a_handler_outlives_the_deadline_is_answered_with_an_internal_error() -> Result<(), Box<dyn Error>> {
        let router = bounded(
            Router::new().route("/ngsi-ld/v1/entities", post(|_body: Bytes| pending::<StatusCode>())),
            Duration::from_millis(20),
        );
        let response = router.oneshot(Request::post("/ngsi-ld/v1/entities").body(Body::from("{}"))?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        Ok(())
    }

    #[tokio::test]
    async fn a_body_arriving_within_the_deadline_keeps_the_handler_answer() -> Result<(), Box<dyn Error>> {
        let response = reading_router(Duration::from_secs(30))
            .oneshot(Request::post("/ngsi-ld/v1/entities").body(Body::from("{}"))?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
        pretty_assertions::assert_eq!(to_bytes(response.into_body(), usize::MAX).await?, Bytes::from_static(b"{}"));
        Ok(())
    }

    #[tokio::test]
    async fn a_request_answered_within_its_deadline_keeps_its_own_answer() -> Result<(), Box<dyn Error>> {
        let router = bounded(Router::new().route("/probe", get(|| async { StatusCode::NO_CONTENT })), Duration::from_secs(30));
        let response = router.oneshot(Request::get("/probe").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::NO_CONTENT);
        Ok(())
    }
}
