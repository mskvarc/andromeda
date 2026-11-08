//! Keeping a connection busy from the start of each request until its response
//! has been sent.
//!
//! A connection is idle only between requests, so its idle time starts when
//! the last response in flight ends, not when the handler returns: a large
//! response is still being written long after that. The response body carries
//! the record of its request, and the HTTP server drops the body once the
//! response has been sent, or abandoned with its connection.

use crate::connection_clock::{ActiveRequest, ConnectionClock};
use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use http_body::{Frame, SizeHint};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

/// Record a request against the connection carrying it, for as long as the
/// request and its response are in flight.
pub(crate) async fn track_request(ConnectInfo(clock): ConnectInfo<ConnectionClock>, request: Request, next: Next) -> Response {
    let active = clock.request_began();
    next.run(request).await.map(|body| Body::new(TrackedBody { body, _active: active }))
}

/// A response body that keeps its request in flight until it is dropped.
struct TrackedBody {
    /// The response body itself.
    body: Body,
    /// The request the body answers, in flight until the body is dropped.
    _active: ActiveRequest,
}

impl HttpBody for TrackedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        Pin::new(&mut self.get_mut().body).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        connection_clock::{ConnectionClock, Phase},
        request_tracking::track_request,
    };
    use axum::{
        Router,
        body::{Body, HttpBody, to_bytes},
        extract::ConnectInfo,
        http::Request,
        middleware::from_fn,
        response::Response,
        routing::get,
    };
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;
    use tower::ServiceExt;

    /// Answer one request on `clock`'s connection with `answered` as the body.
    async fn answer(clock: &ConnectionClock) -> Response {
        let router = Router::new().route("/", get(|| async { "answered" })).layer(from_fn(track_request));
        let mut request = Request::get("/").body(Body::empty()).expect("a valid request");
        request.extensions_mut().insert(ConnectInfo(clock.clone()));
        router.oneshot(request).await.expect("the router answers")
    }

    #[tokio::test]
    async fn the_connection_stays_busy_until_the_response_body_is_dropped() {
        let clock = ConnectionClock::opened();
        let response = answer(&clock).await;
        assert_eq!(clock.phase(), Phase::Busy { requests: NonZeroUsize::MIN });
        drop(response);
        assert!(matches!(clock.phase(), Phase::Idle { .. }), "{:?}", clock.phase());
    }

    #[tokio::test]
    async fn the_tracked_body_keeps_its_length_and_content() {
        let clock = ConnectionClock::opened();
        let response = answer(&clock).await;
        assert_eq!(response.body().size_hint().exact(), Some(8));
        let content = to_bytes(response.into_body(), 64).await.expect("the body is read");
        assert_eq!(content.as_ref(), b"answered");
    }
}
