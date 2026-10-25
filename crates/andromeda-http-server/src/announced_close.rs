//! Whether a response announced that its connection closes after it.
//!
//! A server that answers before it has read a request's content "SHOULD
//! indicate whether it intends to close the connection" (RFC 9110 clause
//! 10.1.1), and over HTTP/1.1 closing is the only way to stop an upload the
//! server refused. A response carrying `Connection: close` is that
//! announcement. Recording it against the connection lets the connection's I/O
//! close in stages ([`crate::staged_close_stream`]) instead of resetting a
//! client that is still sending before it has read the response.

use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use headers::{Connection, HeaderMapExt};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Whether a response on one connection announced that the connection closes
/// after it.
///
/// Every clone shares one record: the connection's I/O holds one, and each
/// request the connection carries receives another as its connect info.
#[derive(Clone, Debug, Default)]
pub(crate) struct AnnouncedClose(Arc<AtomicBool>);

impl AnnouncedClose {
    /// Record that a response announced the close.
    pub(crate) fn announce(&self) {
        // The record guards no other data, so the flag needs no ordering
        // beyond its own.
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether a response on the connection announced the close.
    pub(crate) fn is_announced(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Record a response's `Connection: close` against the connection carrying it.
pub(crate) async fn record_close_announcement(ConnectInfo(announcement): ConnectInfo<AnnouncedClose>, request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if response
        .headers()
        .typed_get::<Connection>()
        .is_some_and(|connection| connection.contains("close"))
    {
        announcement.announce();
    }
    response
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::announced_close::{AnnouncedClose, record_close_announcement};
    use axum::{
        Router,
        body::Body,
        extract::ConnectInfo,
        http::{HeaderValue, Request, header::CONNECTION},
        middleware::from_fn,
        response::IntoResponse,
        routing::get,
    };
    use tower::ServiceExt;

    /// Answer a request on a router that sends `connection` as the response's
    /// `Connection` header, and report whether the connection recorded the
    /// announcement.
    async fn recorded(connection: Option<&'static str>) -> bool {
        let router = Router::new()
            .route(
                "/",
                get(move || async move {
                    match connection {
                        Some(value) => ([(CONNECTION, HeaderValue::from_static(value))], "answered").into_response(),
                        None => "answered".into_response(),
                    }
                }),
            )
            .layer(from_fn(record_close_announcement));
        let announcement = AnnouncedClose::default();
        let mut request = Request::get("/").body(Body::empty()).expect("a valid request");
        request.extensions_mut().insert(ConnectInfo(announcement.clone()));
        router.oneshot(request).await.expect("the router answers");
        announcement.is_announced()
    }

    #[test]
    fn a_new_connection_has_announced_nothing() {
        assert!(!AnnouncedClose::default().is_announced());
    }

    #[test]
    fn an_announcement_is_seen_through_every_clone() {
        let announcement = AnnouncedClose::default();
        let carried = announcement.clone();
        carried.announce();
        assert!(announcement.is_announced());
    }

    #[tokio::test]
    async fn a_response_announcing_the_close_is_recorded() {
        assert!(recorded(Some("close")).await);
    }

    #[tokio::test]
    async fn a_close_among_other_connection_options_is_recorded() {
        assert!(recorded(Some("upgrade, Close")).await);
    }

    #[tokio::test]
    async fn a_keep_alive_response_is_not_recorded() {
        assert!(!recorded(Some("keep-alive")).await);
    }

    #[tokio::test]
    async fn a_response_without_a_connection_header_is_not_recorded() {
        assert!(!recorded(None).await);
    }
}
