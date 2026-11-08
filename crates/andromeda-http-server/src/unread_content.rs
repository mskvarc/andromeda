//! Announcing the close of an HTTP/1.x connection whose request content the
//! response leaves unread.
//!
//! RFC 9110 section 10.1.1: "A server that responds with a final status code
//! before reading the entire request content SHOULD indicate whether it
//! intends to close the connection", and over HTTP/1.x closing it is the only
//! way to stop the upload. Every response is checked here, whichever layer
//! produced it (a `404` or `405` of the routing, a bare `411`, a `415`, a
//! `413`, a `408` or any other refusal), so none of them has to announce the
//! close itself. The announcement lets the connection close in stages
//! ([`crate::staged_close_stream`]), so a client still sending reads the
//! response instead of a reset. HTTP/2 and HTTP/3 stop an unread upload with a
//! stream reset, and RFC 9113 section 8.2.2 forbids the field there.

use axum::{
    body::{Body, Bytes, HttpBody},
    extract::Request,
    http::Version,
    middleware::Next,
    response::Response,
};
use headers::{Connection, HeaderMapExt};
use http_body::{Frame, SizeHint};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

/// Announce `Connection: close` on the response to an HTTP/1.x request whose
/// content was not read to its end by the time the response was ready.
pub(crate) async fn announce_close_of_unread_content(request: Request, next: Next) -> Response {
    if request.version() >= Version::HTTP_2 || request.body().is_end_stream() {
        return next.run(request).await;
    }
    let read_to_end = Arc::new(AtomicBool::new(false));
    let watched = request.map(|inner| {
        Body::new(ReadContent {
            inner,
            read_to_end: Arc::clone(&read_to_end),
        })
    });
    let mut response = next.run(watched).await;
    if !read_to_end.load(Ordering::Acquire) {
        response.headers_mut().typed_insert(Connection::close());
    }
    response
}

/// A request body that records whether it was read to its end.
struct ReadContent {
    /// The body as the connection delivers it.
    inner: Body,
    /// Raised once `inner` has nothing more to deliver.
    read_to_end: Arc<AtomicBool>,
}

impl HttpBody for ReadContent {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let content = self.get_mut();
        let polled = Pin::new(&mut content.inner).poll_frame(cx);
        // A reader may stop at the last frame once the body reports its end,
        // without polling for the end itself.
        if matches!(polled, Poll::Ready(None)) || content.inner.is_end_stream() {
            content.read_to_end.store(true, Ordering::Release);
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use crate::unread_content::announce_close_of_unread_content;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, Version, header::CONNECTION},
        middleware::from_fn,
        routing::post,
    };
    use pretty_assertions::assert_eq;
    use tower::ServiceExt;

    /// A router reading the content of `/read` and refusing `/refuse` unread.
    fn router() -> Router {
        Router::new()
            .route(
                "/read",
                post(|body: Body| async move {
                    match to_bytes(body, usize::MAX).await {
                        Ok(_) => StatusCode::OK,
                        Err(_) => StatusCode::BAD_REQUEST,
                    }
                }),
            )
            .route("/refuse", post(|| async { StatusCode::UNSUPPORTED_MEDIA_TYPE }))
            .layer(from_fn(announce_close_of_unread_content))
    }

    /// The `Connection` field of the answer to a `POST` of `content` to `path`
    /// over `version`.
    async fn connection_field(path: &str, version: Version, content: Body) -> Option<String> {
        let request = Request::post(path).version(version).body(content).expect("a valid request");
        let response = router().oneshot(request).await.expect("the router answers");
        response.headers().get(CONNECTION).and_then(|value| value.to_str().ok()).map(str::to_owned)
    }

    #[tokio::test]
    async fn a_refusal_leaving_the_content_unread_announces_the_close() {
        assert_eq!(connection_field("/refuse", Version::HTTP_11, Body::from("{}")).await.as_deref(), Some("close"));
    }

    #[tokio::test]
    async fn a_route_that_does_not_exist_announces_the_close() {
        assert_eq!(
            connection_field("/nothing-here", Version::HTTP_11, Body::from("{}")).await.as_deref(),
            Some("close")
        );
    }

    #[tokio::test]
    async fn content_read_to_its_end_leaves_the_connection_open() {
        assert_eq!(connection_field("/read", Version::HTTP_11, Body::from("{}")).await, None);
    }

    #[tokio::test]
    async fn a_request_without_content_leaves_the_connection_open() {
        assert_eq!(connection_field("/refuse", Version::HTTP_11, Body::empty()).await, None);
    }

    #[tokio::test]
    async fn an_http2_refusal_never_carries_the_field() {
        assert_eq!(connection_field("/refuse", Version::HTTP_2, Body::from("{}")).await, None);
    }
}
