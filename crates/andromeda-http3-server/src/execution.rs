//! Serving an assembled axum router over an HTTP/3 (QUIC) endpoint.
//!
//! The bound endpoint is driven by [`axum_h3::H3Router`], which accepts each
//! QUIC connection, then each request on it, and dispatches the request to the
//! same axum router the TCP listener serves. Serving ends when the supplied
//! shutdown future resolves, after which in-flight connections are given until
//! the endpoint goes idle to finish.

use axum::Router;
use axum_h3::H3Router;
use h3_util::quinn::H3QuinnAcceptor;
use quinn::Endpoint;
use std::{future::Future, io};
use thiserror::Error;

/// Failure while serving HTTP/3.
#[derive(Debug, Error)]
pub enum Http3ServeError {
    /// The bound endpoint's local address could not be read.
    #[error("local address of the QUIC endpoint is unavailable: {0}")]
    LocalAddress(#[source] io::Error),
    /// The HTTP/3 connection accept loop terminated with an error.
    ///
    /// The underlying `h3` failure is type-erased upstream, so its human-readable
    /// rendering is captured here rather than the opaque error itself.
    #[error("HTTP/3 serving failed: {message}")]
    Serve {
        /// The rendered cause of the serving failure.
        message: String,
    },
}

/// Serve `router` over the bound QUIC `endpoint` until `shutdown` resolves.
///
/// After the shutdown future resolves the accept loop stops taking new
/// connections and the endpoint is awaited idle, letting connections already in
/// flight complete before the function returns.
///
/// # Errors
///
/// Returns [`Http3ServeError::LocalAddress`] when the endpoint's address cannot
/// be read for logging, and [`Http3ServeError::Serve`] when the accept loop
/// fails.
pub async fn serve_endpoint<F>(endpoint: Endpoint, router: Router, shutdown: F) -> Result<(), Http3ServeError>
where
    F: Future<Output = ()> + Send,
{
    let local_address = endpoint.local_addr().map_err(Http3ServeError::LocalAddress)?;
    tracing::info!(address = %local_address, "HTTP/3 listener accepting connections");
    // The acceptor takes ownership of the endpoint handle; a second cheap,
    // Arc-backed handle is retained so the endpoint can be awaited idle once
    // serving stops.
    let acceptor = H3QuinnAcceptor::new(endpoint.clone());
    let result = H3Router::from(router).serve_with_shutdown(acceptor, shutdown).await;
    endpoint.wait_idle().await;
    result.map_err(|error| Http3ServeError::Serve { message: error.to_string() })
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint::bind,
        execution::{Http3ServeError, serve_endpoint},
        test_support::{H3_ALPN, Http3Client, SelfSignedListener, request},
    };
    use axum::{
        Router,
        extract::Query,
        routing::{get, post, put},
    };
    use bytes::Bytes;
    use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
    use rustls::RootCertStore;
    use std::{
        collections::HashMap,
        error::Error,
        net::{Ipv4Addr, SocketAddr},
        sync::Arc,
    };
    use tokio::{
        sync::{Notify, oneshot},
        task::JoinHandle,
    };
    use tower::Layer;
    use tower_http::normalize_path::NormalizePathLayer;

    /// A router exercising the request and response shapes an HTTP/3 client cares about.
    fn test_router() -> Router {
        Router::new()
            .route("/probe", get(|| async { "h3 works" }))
            .route("/status", get(|| async { StatusCode::NO_CONTENT }))
            .route("/echo", post(|body: Bytes| async move { body }))
            .route("/large", get(|| async { "x".repeat(200_000) }))
            // Echo the `x-probe` request header back as `x-echo`, proving request
            // headers reach the handler and response headers return over HTTP/3.
            .route(
                "/header-echo",
                get(|headers: HeaderMap| async move {
                    let value = headers.get("x-probe").cloned().unwrap_or_else(|| HeaderValue::from_static("absent"));
                    ([(HeaderName::from_static("x-echo"), value)], "ok")
                }),
            )
            .route(
                "/query",
                get(|Query(params): Query<HashMap<String, String>>| async move { params.get("name").cloned().unwrap_or_default() }),
            )
            .route("/method", put(|| async { "put" }).delete(|| async { "delete" }).patch(|| async { "patch" }))
    }

    /// The router wrapped exactly as [`crate::config`] documents for HTTP/3:
    /// trailing-slash normalization applied outside routing via a fallback.
    fn normalized_router() -> Router {
        Router::new().fallback_service(NormalizePathLayer::trim_trailing_slash().layer(test_router()))
    }

    /// A listener bound to an ephemeral port and serving a router in the background.
    struct RunningListener {
        listener: SelfSignedListener,
        address: SocketAddr,
        shutdown: Option<oneshot::Sender<()>>,
        handle: JoinHandle<Result<(), Http3ServeError>>,
    }

    impl RunningListener {
        /// Start serving `router` on a fresh self-signed listener.
        ///
        /// Not async: it binds and spawns without awaiting, but must be called
        /// from within a Tokio runtime (every caller is a `#[tokio::test]`).
        fn start(router: Router) -> Result<RunningListener, Box<dyn Error + Send + Sync>> {
            let listener = SelfSignedListener::generate()?;
            let endpoint = {
                let activation = listener.config().activation()?.expect("enabled config yields activation");
                bind(activation, SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?
            };
            let address = endpoint.local_addr()?;
            let (shutdown_sender, shutdown_receiver) = oneshot::channel();
            let handle = tokio::spawn(serve_endpoint(endpoint, router, async move {
                let _ = shutdown_receiver.await;
            }));
            Ok(RunningListener {
                listener,
                address,
                shutdown: Some(shutdown_sender),
                handle,
            })
        }

        /// The port the listener is bound to.
        fn port(&self) -> u16 {
            self.address.port()
        }

        /// Dial the listener over HTTP/3.
        async fn dial(&self) -> Result<Http3Client, Box<dyn Error + Send + Sync>> {
            self.listener.dial(self.address).await
        }

        /// Signal shutdown and wait for the server to drain and stop.
        async fn stop(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
            if let Some(sender) = self.shutdown.take() {
                let _ = sender.send(());
            }
            self.handle.await??;
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_get_returns_its_body_and_ok() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request(Method::GET, running.port(), "/probe")?, &[]).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.text()?, "h3 works");
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn a_status_only_response_carries_no_body() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request(Method::GET, running.port(), "/status")?, &[]).await?;
        assert_eq!(response.status, StatusCode::NO_CONTENT);
        assert!(response.body.is_empty(), "a 204 carries no body");
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn a_post_body_is_echoed_back() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request(Method::POST, running.port(), "/echo")?, b"round-trip body").await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.text()?, "round-trip body");
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn a_large_body_spanning_multiple_frames_is_received_whole() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request(Method::GET, running.port(), "/large")?, &[]).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body.len(), 200_000, "the whole streamed body is reassembled");
        assert!(response.body.iter().all(|byte| *byte == b'x'));
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn a_request_header_reaches_the_handler_and_a_response_header_returns() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let uri = format!("https://localhost:{}/header-echo", running.port());
        let request = Request::builder().method(Method::GET).uri(uri).header("x-probe", "seen").body(())?;
        let response = client.send(request, &[]).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.headers.get("x-echo").and_then(|value| value.to_str().ok()), Some("seen"));
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn a_query_parameter_reaches_the_handler() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request(Method::GET, running.port(), "/query?name=quic")?, &[]).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.text()?, "quic");
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn an_unknown_route_returns_not_found() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request(Method::GET, running.port(), "/nonexistent")?, &[]).await?;
        assert_eq!(response.status, StatusCode::NOT_FOUND);
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn put_delete_and_patch_each_reach_their_handler() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        for (method, expected) in [(Method::PUT, "put"), (Method::DELETE, "delete"), (Method::PATCH, "patch")] {
            let response = client.send(request(method.clone(), running.port(), "/method")?, &[]).await?;
            assert_eq!(response.status, StatusCode::OK, "{method} should be served");
            assert_eq!(response.text()?, expected, "{method} should reach its handler");
        }
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn a_trailing_slash_is_normalized_over_http3() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(normalized_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request(Method::GET, running.port(), "/probe/")?, &[]).await?;
        assert_eq!(response.status, StatusCode::OK, "a trailing slash should be trimmed before routing");
        assert_eq!(response.text()?, "h3 works");
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn several_requests_reuse_one_connection() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        for _ in 0..3 {
            let response = client.send(request(Method::GET, running.port(), "/probe")?, &[]).await?;
            assert_eq!(response.status, StatusCode::OK);
            assert_eq!(response.text()?, "h3 works");
        }
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn concurrent_request_streams_are_served_on_one_connection() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let client = running.dial().await?;
        let first = Http3Client::exchange(client.opener(), request(Method::GET, running.port(), "/probe")?, &[]);
        let second = Http3Client::exchange(client.opener(), request(Method::POST, running.port(), "/echo")?, b"second");
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first?.text()?, "h3 works");
        assert_eq!(second?.text()?, "second");
        client.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn two_independent_connections_are_served() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut first = running.dial().await?;
        let mut second = running.dial().await?;
        assert_eq!(first.send(request(Method::GET, running.port(), "/probe")?, &[]).await?.text()?, "h3 works");
        assert_eq!(second.send(request(Method::GET, running.port(), "/probe")?, &[]).await?.text()?, "h3 works");
        first.close().await;
        second.close().await;
        running.stop().await
    }

    #[tokio::test]
    async fn an_in_flight_request_completes_after_graceful_shutdown() -> Result<(), Box<dyn Error + Send + Sync>> {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let handler_started = Arc::clone(&started);
        let handler_release = Arc::clone(&release);
        let router = Router::new().route(
            "/slow",
            get(move || {
                let started = Arc::clone(&handler_started);
                let release = Arc::clone(&handler_release);
                async move {
                    started.notify_one();
                    release.notified().await;
                    "drained"
                }
            }),
        );

        let listener = SelfSignedListener::generate()?;
        let endpoint = {
            let activation = listener.config().activation()?.expect("enabled config yields activation");
            bind(activation, SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?
        };
        let address = endpoint.local_addr()?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_endpoint(endpoint, router, async move {
            let _ = shutdown_receiver.await;
        }));

        let mut client = listener.dial(address).await?;
        // Signal shutdown and release the handler once the request is in flight,
        // then confirm the response still completes.
        let coordinator = tokio::spawn({
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            async move {
                started.notified().await;
                let _ = shutdown_sender.send(());
                release.notify_one();
            }
        });

        let response = client.send(request(Method::GET, address.port(), "/slow")?, &[]).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.text()?, "drained", "an in-flight request drains after shutdown begins");
        coordinator.await?;
        client.close().await;
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn a_client_not_trusting_the_certificate_is_rejected() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let outcome = Http3Client::connect(RootCertStore::empty(), vec![H3_ALPN.to_vec()], running.address).await;
        assert!(outcome.is_err(), "a client trusting no roots must not complete the handshake");
        running.stop().await
    }

    #[tokio::test]
    async fn a_client_offering_a_non_h3_alpn_is_rejected() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let roots = running.listener.root_store()?;
        let outcome = Http3Client::connect(roots, vec![b"h2".to_vec()], running.address).await;
        assert!(outcome.is_err(), "a client sharing no ALPN with the server must not complete the handshake");
        running.stop().await
    }
}
