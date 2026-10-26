//! Binding and serving an already assembled state-complete HTTP service.

use crate::{
    announced_close::record_close_announcement,
    close_policy::ClosePolicy,
    connection_builder::connection_builder,
    error::HttpServerError,
    field_section::{HeaderSectionPolicy, refuse_oversized_http1_section},
    listener_security::ListenerSecurity,
    staged_close_listener::StagedCloseListener,
    tls_tcp_listener::TlsTcpListener,
};
use andromeda_configuration::schema::server::ServerConfig;
use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    middleware::{from_fn, from_fn_with_state},
    response::Response,
    serve::Listener,
};
use hyper::body::Incoming;
use hyper_util::{rt::TokioIo, server::graceful::GracefulShutdown, service::TowerToHyperService};
use std::{convert::Infallible, future::Future, net::SocketAddr};
use tokio::{net::TcpListener, time::timeout};
use tower::{Layer, Service, ServiceExt};

/// Bind the configured listener and serve until the supplied shutdown future resolves.
///
/// `security` decides whether the bound socket serves plaintext or terminates
/// TLS; composition derives it from the listener's configured transport.
///
/// # Errors
///
/// Returns [`HttpServerError::Bind`] when the configured address cannot be bound,
/// [`HttpServerError::LocalAddress`] when the bound listener's effective address cannot be read,
/// and [`HttpServerError::DrainDeadlineExceeded`] when accepted connections are still in flight
/// once the configured graceful-drain deadline has passed.
pub async fn serve<S, F>(config: &ServerConfig, security: ListenerSecurity, service: S, shutdown: F) -> Result<(), HttpServerError>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    F: Future<Output = ()> + Send,
{
    let address = SocketAddr::from((config.host, config.port));
    let listener = TcpListener::bind(address).await.map_err(|source| HttpServerError::Bind { address, source })?;
    serve_bound(config, listener, security, service, shutdown).await
}

/// Serve an already bound TCP `listener` under `config`'s policies until the
/// supplied shutdown future resolves; `config`'s host and port are not read.
///
/// # Errors
///
/// Returns [`HttpServerError::LocalAddress`] when the listener's effective address cannot be read,
/// and [`HttpServerError::DrainDeadlineExceeded`] when accepted connections are still in flight
/// once the configured graceful-drain deadline has passed.
pub async fn serve_bound<S, F>(config: &ServerConfig, listener: TcpListener, security: ListenerSecurity, service: S, shutdown: F) -> Result<(), HttpServerError>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    F: Future<Output = ()> + Send,
{
    let local_address = listener.local_addr().map_err(|source| HttpServerError::LocalAddress { source })?;
    tracing::info!(address = %local_address, transport = %security.transport(), "HTTP listener accepting connections");
    let policy = ClosePolicy::from(config);
    let header_section = HeaderSectionPolicy {
        limit: config.max_request_header_section,
        transport: security.transport(),
    };
    match security {
        ListenerSecurity::Plaintext => serve_listener(listener, service, shutdown, policy, header_section).await,
        ListenerSecurity::Tls { feed, handshake_timeout } => {
            serve_listener(
                TlsTcpListener::new(listener, feed, handshake_timeout),
                service,
                shutdown,
                policy,
                header_section,
            )
            .await
        }
    }
}

/// Serve an already bound listener, allowing composition and tests to control its address.
///
/// Each connection speaks HTTP/1.1 or HTTP/2 as the client chooses, and a
/// request whose header section is over `header_section`'s limit is answered
/// 431 on either. A connection on which a response announced `Connection:
/// close` closes in stages, delayed by the policy's announced-close delay (see
/// [`crate::staged_close_stream`]); every other connection closes as soon as
/// it is done. Once `shutdown` resolves no connection is accepted, and the
/// open ones finish their in-flight requests.
///
/// # Errors
///
/// Returns [`HttpServerError::DrainDeadlineExceeded`] when accepted connections are still in
/// flight once the policy's drain deadline has passed.
pub async fn serve_listener<L, S, F>(
    listener: L,
    service: S,
    shutdown: F,
    policy: ClosePolicy,
    header_section: HeaderSectionPolicy,
) -> Result<(), HttpServerError>
where
    L: Listener,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    F: Future<Output = ()> + Send,
{
    let builder = connection_builder(header_section.limit);
    let service = from_fn(record_close_announcement).layer(from_fn_with_state(header_section, refuse_oversized_http1_section).layer(service));
    let mut listener = StagedCloseListener::new(listener, policy.announced_close_delay);
    let graceful = GracefulShutdown::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            (io, _address) = listener.accept() => {
                // clone: the connection's I/O and every request it carries share one record.
                let announcement = io.announcement().clone();
                // clone: each connection owns a handle to the shared service.
                let connection_service = service.clone().map_request(move |request: Request<Incoming>| {
                    let mut request = request.map(Body::new);
                    request.extensions_mut().insert(ConnectInfo(announcement.clone()));
                    request
                });
                let connection = builder.serve_connection(TokioIo::new(io), TowerToHyperService::new(connection_service)).into_owned();
                let watched = graceful.watch(connection);
                tokio::spawn(async move {
                    if let Err(error) = watched.await {
                        tracing::trace!(error = %error, "HTTP connection ended with an error");
                    }
                });
            }
            () = &mut shutdown => break,
        }
    }
    drop(listener);
    timeout(policy.drain_deadline, graceful.shutdown())
        .await
        .map_err(|_elapsed| HttpServerError::DrainDeadlineExceeded {
            deadline: policy.drain_deadline,
        })
}

#[cfg(test)]
mod tests {
    use crate::{
        close_policy::ClosePolicy,
        error::HttpServerError,
        execution::serve_listener,
        field_section::{HeaderSectionPolicy, field_section_size},
    };
    use andromeda_configuration::{header_section_limit::HeaderSectionLimit, schema::listener_transport::ListenerTransport};
    use axum::{
        Router,
        extract::Request,
        http::{StatusCode, header::CONNECTION},
        routing::{get, post},
    };
    use std::{io, net::SocketAddr, num::ParseIntError, sync::Arc, time::Duration};
    use tokio::{
        net::TcpListener,
        sync::{Notify, oneshot},
        task::{JoinError, JoinHandle},
    };

    #[derive(Debug, thiserror::Error)]
    enum TestError {
        #[error(transparent)]
        Io(#[from] io::Error),
        #[error(transparent)]
        Request(#[from] reqwest::Error),
        #[error(transparent)]
        Server(#[from] HttpServerError),
        #[error(transparent)]
        Task(#[from] JoinError),
        #[error(transparent)]
        Measure(#[from] ParseIntError),
        #[error("shutdown receiver was dropped")]
        Shutdown,
    }

    /// The policy every fixture server runs under unless a test says otherwise.
    const POLICY: ClosePolicy = ClosePolicy {
        drain_deadline: Duration::from_secs(2),
        announced_close_delay: Duration::from_millis(500),
    };

    /// The header section policy every fixture server runs under: the
    /// smallest limit, on a plaintext listener.
    const HEADER_SECTION: HeaderSectionPolicy = HeaderSectionPolicy {
        limit: HeaderSectionLimit::MINIMUM,
        transport: ListenerTransport::Plaintext,
    };

    /// The size of an upload the refusing fixture router never reads.
    const REFUSED_UPLOAD: usize = 16 * 1024 * 1024;

    async fn ephemeral_listener() -> Result<(TcpListener, SocketAddr), io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        Ok((listener, address))
    }

    /// A router that refuses every upload before reading a byte of it, and
    /// announces that it closes the connection, as the broker's payload reader
    /// does for a body over its limit.
    fn refusing_router() -> Router {
        Router::new().route(
            "/upload",
            post(|| async { (StatusCode::PAYLOAD_TOO_LARGE, [(CONNECTION, "close")], "refused") }),
        )
    }

    #[tokio::test]
    async fn generic_server_serves_a_router_unrelated_to_ngsi_ld() -> Result<(), TestError> {
        let (listener, address) = ephemeral_listener().await?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_listener(
            listener,
            Router::new().route("/test", get(|| async { "served" })),
            async move {
                match shutdown_receiver.await {
                    Ok(()) | Err(_) => {}
                }
            },
            POLICY,
            HEADER_SECTION,
        ));
        let response = reqwest::get(format!("http://{address}/test")).await?;
        pretty_assertions::assert_eq!(response.text().await?, "served");
        shutdown_sender.send(()).map_err(|()| TestError::Shutdown)?;
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn graceful_shutdown_waits_for_an_in_flight_request_to_complete() -> Result<(), TestError> {
        let (listener, address) = ephemeral_listener().await?;
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
                    "complete"
                }
            }),
        );
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_listener(
            listener,
            router,
            async move {
                match shutdown_receiver.await {
                    Ok(()) | Err(_) => {}
                }
            },
            POLICY,
            HEADER_SECTION,
        ));
        let request = tokio::spawn(reqwest::get(format!("http://{address}/slow")));
        started.notified().await;
        shutdown_sender.send(()).map_err(|()| TestError::Shutdown)?;
        release.notify_one();
        let response = request.await??;
        pretty_assertions::assert_eq!(response.text().await?, "complete");
        server.await??;
        Ok(())
    }

    /// RFC 9112 clause 9.6: a refusal answered before the upload is read, on a
    /// connection closed at once, is reset under an HTTP/1.1 client that is
    /// still writing, and hyper's client then reports the failed write instead
    /// of the refusal. Closed in stages, every refusal reaches the client.
    #[tokio::test]
    async fn an_early_refusal_reaches_an_http1_client_still_uploading() -> Result<(), TestError> {
        let (listener, address) = ephemeral_listener().await?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_listener(
            listener,
            refusing_router(),
            async move {
                match shutdown_receiver.await {
                    Ok(()) | Err(_) => {}
                }
            },
            POLICY,
            HEADER_SECTION,
        ));
        let upload = vec![b' '; REFUSED_UPLOAD];
        for _ in 0..20 {
            // A client per upload, as each refused connection is closed.
            let response = reqwest::Client::new()
                .post(format!("http://{address}/upload"))
                .body(upload.clone()) // clone: every request owns its payload
                .send()
                .await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
            pretty_assertions::assert_eq!(response.text().await?, "refused");
        }
        shutdown_sender.send(()).map_err(|()| TestError::Shutdown)?;
        server.await??;
        Ok(())
    }

    /// A connection delaying its announced close finishes inside the graceful
    /// drain, so shutdown is never failed by it.
    #[tokio::test]
    async fn a_delayed_close_finishes_inside_the_graceful_drain() -> Result<(), TestError> {
        let (listener, address) = ephemeral_listener().await?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_listener(
            listener,
            refusing_router(),
            async move {
                match shutdown_receiver.await {
                    Ok(()) | Err(_) => {}
                }
            },
            POLICY,
            HEADER_SECTION,
        ));
        let response = reqwest::Client::new()
            .post(format!("http://{address}/upload"))
            .body(vec![b' '; REFUSED_UPLOAD])
            .send()
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        shutdown_sender.send(()).map_err(|()| TestError::Shutdown)?;
        server.await??;
        Ok(())
    }

    /// A router serving `/` and reporting, on `/measure`, the size of the
    /// header section it received.
    fn measuring_router() -> Router {
        Router::new().route("/", get(|| async { "served" })).route(
            "/measure",
            get(|request: Request| async move { field_section_size(&request, ListenerTransport::Plaintext).to_string() }),
        )
    }

    /// Serve the measuring router until the returned sender fires.
    async fn measuring_server() -> Result<(SocketAddr, oneshot::Sender<()>, JoinHandle<Result<(), HttpServerError>>), TestError> {
        let (listener, address) = ephemeral_listener().await?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_listener(
            listener,
            measuring_router(),
            async move {
                match shutdown_receiver.await {
                    Ok(()) | Err(_) => {}
                }
            },
            POLICY,
            HEADER_SECTION,
        ));
        Ok((address, shutdown_sender, server))
    }

    /// The `x-fill` value length that brings `client`'s request for `/` to
    /// exactly the limit, from the size the server measures for a one-octet
    /// fill.
    async fn filling_length(client: &reqwest::Client, address: SocketAddr) -> Result<usize, TestError> {
        let probe = client.get(format!("http://{address}/measure")).header("x-fill", "a").send().await?;
        let measured: usize = probe.text().await?.parse()?;
        // `/measure` is seven octets longer than `/`.
        Ok(1 + usize::from(HEADER_SECTION.limit.octets()) - (measured - 7))
    }

    /// Request `/` with an `x-fill` field of `length` octets.
    async fn filled(client: &reqwest::Client, address: SocketAddr, length: usize) -> Result<(StatusCode, String), TestError> {
        let response = client.get(format!("http://{address}/")).header("x-fill", "a".repeat(length)).send().await?;
        Ok((response.status(), response.text().await?))
    }

    /// RFC 9113 section 6.5.2: the section at the limit is served and one
    /// octet over it is refused, on HTTP/1.1 by the broker's measure and on
    /// HTTP/2 by the protocol's, so both measure alike.
    #[tokio::test]
    async fn a_header_section_at_the_limit_is_served_and_one_octet_over_refused_431_over_http1_and_http2() -> Result<(), TestError> {
        let (address, shutdown_sender, server) = measuring_server().await?;
        for client in [
            reqwest::Client::builder().http1_only().build()?,
            reqwest::Client::builder().http2_prior_knowledge().build()?,
        ] {
            let length = filling_length(&client, address).await?;
            assert_eq!(filled(&client, address, length).await?, (StatusCode::OK, "served".to_owned()));
            assert_eq!(
                filled(&client, address, length + 1).await?,
                (StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE, String::new())
            );
        }
        shutdown_sender.send(()).map_err(|()| TestError::Shutdown)?;
        server.await??;
        Ok(())
    }

    /// A head larger than the limit fills the HTTP/1.1 read buffer before it
    /// is parsed whole, and is refused there.
    #[tokio::test]
    async fn a_head_larger_than_the_read_buffer_is_refused_431_over_http1() -> Result<(), TestError> {
        let (address, shutdown_sender, server) = measuring_server().await?;
        let client = reqwest::Client::builder().http1_only().build()?;
        assert_eq!(filled(&client, address, 64 * 1024).await?.0, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
        shutdown_sender.send(()).map_err(|()| TestError::Shutdown)?;
        server.await??;
        Ok(())
    }

    /// The HTTP/1.1 parser's own count of header lines stops at 100 unless
    /// raised; 150 small fields fit an 8 KiB section.
    #[tokio::test]
    async fn one_hundred_and_fifty_small_fields_within_the_limit_are_served_over_http1() -> Result<(), TestError> {
        let (address, shutdown_sender, server) = measuring_server().await?;
        let client = reqwest::Client::builder().http1_only().build()?;
        let request = (0..150).fold(client.get(format!("http://{address}/")), |request, index| {
            request.header(format!("x-{index}"), "v")
        });
        let response = request.send().await?;
        assert_eq!(response.status(), StatusCode::OK);
        shutdown_sender.send(()).map_err(|()| TestError::Shutdown)?;
        server.await??;
        Ok(())
    }
}
