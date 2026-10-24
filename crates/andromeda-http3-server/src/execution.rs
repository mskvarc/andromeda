//! Serving an assembled axum router over an HTTP/3 (QUIC) endpoint.
//!
//! The bound endpoint is driven by [`crate::request_loop`], which accepts each
//! QUIC connection, then each request on it, and dispatches the request to the
//! same axum router the TCP listener serves. While it serves, every
//! configuration the shared TLS identity's feed rebuilds -- a rotated
//! certificate, a changed client CA -- is installed on the endpoint, so new
//! connections present it while open ones keep the session they negotiated.
//! Serving ends when the supplied shutdown future resolves, after which
//! in-flight connections are given until the endpoint goes idle to finish.

use crate::{quic_crypto::quic_server_config, request_loop::accept_connections};
use andromeda_configuration_scalars::header_section_limit::HeaderSectionLimit;
use andromeda_server_tls::server_config_feed::ServerConfigFeed;
use axum::Router;
use h3_util::quinn::H3QuinnAcceptor;
use quinn::Endpoint;
use std::{
    convert::Infallible,
    future::{Future, pending},
    io,
};
use thiserror::Error;

/// Failure while serving HTTP/3.
#[derive(Debug, Error)]
pub enum Http3ServeError {
    /// The bound endpoint's local address could not be read.
    #[error("local address of the QUIC endpoint is unavailable: {0}")]
    LocalAddress(#[source] io::Error),
    /// The HTTP/3 connection accept loop terminated with an error.
    ///
    /// The QUIC acceptor hands the failure back type-erased, so there is no
    /// typed cause to chain; the erased error is logged where it surfaces
    /// instead.
    #[error("HTTP/3 connection accept loop failed")]
    Serve,
}

/// Serve `router` over the bound QUIC `endpoint` until `shutdown` resolves,
/// installing each configuration `feed` rebuilds as it arrives.
///
/// A request whose header section is over `limit` never reaches the router
/// (see [`crate::request_loop`]).
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
pub async fn serve_endpoint<F>(
    endpoint: Endpoint,
    feed: ServerConfigFeed,
    router: Router,
    limit: HeaderSectionLimit,
    shutdown: F,
) -> Result<(), Http3ServeError>
where
    F: Future<Output = ()> + Send,
{
    let local_address = endpoint.local_addr().map_err(Http3ServeError::LocalAddress)?;
    tracing::info!(address = %local_address, "HTTP/3 listener accepting connections");
    // The acceptor takes ownership of the endpoint handle; a second cheap,
    // Arc-backed handle is retained to install rotated configurations and to
    // await the endpoint idle once serving stops.
    let acceptor = H3QuinnAcceptor::new(endpoint.clone());
    let result = tokio::select! {
        result = accept_connections(acceptor, router, limit, shutdown) => result,
        never = install_rotations(&endpoint, feed) => match never {},
    };
    endpoint.wait_idle().await;
    result.map_err(|error| {
        tracing::error!(error = %error, "HTTP/3 connection accept loop failed");
        Http3ServeError::Serve
    })
}

/// Install every configuration `feed` rebuilds on `endpoint`, for as long as
/// the endpoint serves.
///
/// A configuration quinn rejects is logged and the endpoint keeps the one it
/// has. Once the feed can no longer change the endpoint keeps its last
/// configuration, and this waits out the rest of serving.
async fn install_rotations(endpoint: &Endpoint, mut feed: ServerConfigFeed) -> Infallible {
    while let Some(tls) = feed.changed().await {
        match quic_server_config(tls) {
            Ok(config) => {
                endpoint.set_server_config(Some(config));
                tracing::info!(
                    event = "quic_tls_config_installed",
                    "HTTP/3 listener presents the reloaded TLS material to new connections"
                );
            }
            Err(error) => tracing::error!(
                event = "quic_tls_config_rejected",
                error = %error,
                "keeping the previous HTTP/3 TLS configuration"
            ),
        }
    }
    pending().await
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint::bind,
        execution::{Http3ServeError, serve_endpoint},
        http3_client::{Http3Client, Http3ClientError, request},
        quic_crypto::H3_ALPN,
        test_support::QuicIdentity,
    };
    use andromeda_configuration_scalars::header_section_limit::HeaderSectionLimit;
    use andromeda_server_tls::test_identity::authority::TestAuthority;
    use axum::{
        Router,
        extract::Query,
        routing::{get, post, put},
    };
    use bytes::Bytes;
    use h3::error::StreamError;
    use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
    use rustls::RootCertStore;
    use std::{
        collections::HashMap,
        error::Error,
        net::{Ipv4Addr, SocketAddr},
        sync::Arc,
        time::{Duration, Instant},
    };
    use tokio::{
        sync::{Notify, oneshot},
        task::JoinHandle,
        time::sleep,
    };
    use tower::Layer;
    use tower_http::normalize_path::NormalizePathLayer;

    /// The request header section limit every fixture listener runs under.
    const LIMIT: HeaderSectionLimit = HeaderSectionLimit::MINIMUM;

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
        identity: QuicIdentity,
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
            RunningListener::serve(QuicIdentity::anonymous()?, router)
        }

        /// Start serving `router` with the given identity.
        fn serve(identity: QuicIdentity, router: Router) -> Result<RunningListener, Box<dyn Error + Send + Sync>> {
            let endpoint = bind(identity.feed(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
            let address = endpoint.local_addr()?;
            let (shutdown_sender, shutdown_receiver) = oneshot::channel();
            let handle = tokio::spawn(serve_endpoint(endpoint, identity.feed().clone(), router, LIMIT, async move {
                let _ = shutdown_receiver.await;
            }));
            Ok(RunningListener {
                identity,
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
            self.identity.dial(self.address).await
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

        let identity = QuicIdentity::anonymous()?;
        let endpoint = bind(identity.feed(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let address = endpoint.local_addr()?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_endpoint(endpoint, identity.feed().clone(), router, LIMIT, async move {
            let _ = shutdown_receiver.await;
        }));

        let mut client = identity.dial(address).await?;
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
        let outcome = Http3Client::connect(RootCertStore::empty(), vec![H3_ALPN.to_vec()], running.address, None).await;
        assert!(outcome.is_err(), "a client trusting no roots must not complete the handshake");
        running.stop().await
    }

    #[tokio::test]
    async fn a_client_offering_a_non_h3_alpn_is_rejected() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let roots = running.identity.root_store()?;
        let outcome = Http3Client::connect(roots, vec![b"h2".to_vec()], running.address, None).await;
        assert!(outcome.is_err(), "a client sharing no ALPN with the server must not complete the handshake");
        running.stop().await
    }

    #[tokio::test]
    async fn the_endpoint_presents_a_rotated_certificate_to_new_connections() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut open = running.dial().await?;
        let original_leaf = open.server_leaf().cloned();
        let rotated = running.identity.rotate()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut fresh = running.dial().await?;
        while fresh.server_leaf() != Some(rotated.certificate()) {
            assert!(Instant::now() < deadline, "the rotated certificate was never presented");
            fresh.close().await;
            sleep(Duration::from_millis(50)).await;
            fresh = running.dial().await?;
        }
        assert_ne!(original_leaf.as_ref(), Some(rotated.certificate()));
        let response = fresh.send(request(Method::GET, running.port(), "/probe")?, &[]).await?;
        assert_eq!(response.text()?, "h3 works");
        let survived = open.send(request(Method::GET, running.port(), "/probe")?, &[]).await?;
        assert_eq!(survived.text()?, "h3 works", "the connection opened before the rotation keeps serving");
        assert_eq!(open.server_leaf().cloned(), original_leaf);
        fresh.close().await;
        open.close().await;
        running.stop().await
    }

    /// The QUIC endpoint verifies client certificates through the same shared
    /// rustls configuration the TCP listener uses.
    #[tokio::test]
    async fn required_client_authentication_rejects_a_quic_client_without_a_certificate() -> Result<(), Box<dyn Error + Send + Sync>> {
        let clients = TestAuthority::generate()?;
        let running = RunningListener::serve(QuicIdentity::requiring_clients_of(&clients)?, test_router())?;
        let anonymous = async {
            let mut client = running.dial().await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(client.send(request(Method::GET, running.port(), "/probe")?, &[]).await?)
        };
        assert!(anonymous.await.is_err(), "a client without a certificate is refused");
        let client_identity = clients.issue_client_identity()?;
        let mut authenticated = Http3Client::connect(running.identity.root_store()?, vec![H3_ALPN.to_vec()], running.address, Some(&client_identity)).await?;
        let response = authenticated.send(request(Method::GET, running.port(), "/probe")?, &[]).await?;
        assert_eq!(response.text()?, "h3 works");
        authenticated.close().await;
        running.stop().await
    }

    #[test]
    fn a_failed_accept_loop_is_reported_without_an_erased_cause() {
        assert_eq!(Http3ServeError::Serve.to_string(), "HTTP/3 connection accept loop failed");
        assert!(
            Http3ServeError::Serve.source().is_none(),
            "the upstream cause is type-erased and logged, not chained"
        );
    }

    /// A request for `/probe` whose `x-fill` field brings its header section,
    /// as RFC 9114 section 4.2.2 measures it, to `size` octets.
    fn request_of_section_size(port: u16, size: usize) -> Result<Request<()>, Box<dyn Error + Send + Sync>> {
        let authority = format!("localhost:{port}");
        let pseudo = (7 + 3 + 32) + (7 + 5 + 32) + (10 + authority.len() + 32) + (5 + "/probe".len() + 32);
        let fill = size - pseudo - (6 + 32);
        Ok(Request::builder()
            .method(Method::GET)
            .uri(format!("https://{authority}/probe"))
            .header("x-fill", "a".repeat(fill))
            .body(())?)
    }

    #[tokio::test]
    async fn a_request_whose_field_section_is_exactly_the_limit_is_served() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        let response = client.send(request_of_section_size(running.port(), usize::from(LIMIT.octets()))?, &[]).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.text()?, "h3 works");
        client.close().await;
        running.stop().await
    }

    /// RFC 9114 section 7.2.4.2: a client that has read the advertised limit
    /// refuses to send a section over it; one that has not yet read it sends
    /// the request and the listener answers 431. Either way it never reaches
    /// the router.
    #[tokio::test]
    async fn a_request_one_octet_over_the_limit_is_refused() -> Result<(), Box<dyn Error + Send + Sync>> {
        let running = RunningListener::start(test_router())?;
        let mut client = running.dial().await?;
        match client
            .send(request_of_section_size(running.port(), usize::from(LIMIT.octets()) + 1)?, &[])
            .await
        {
            Ok(response) => assert_eq!(response.status, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE),
            Err(error) => assert!(
                matches!(&error, Http3ClientError::Stream(StreamError::HeaderTooBig { max_size, .. }) if *max_size == u64::from(LIMIT.octets())),
                "{error}"
            ),
        }
        client.close().await;
        running.stop().await
    }
}
