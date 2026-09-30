//! Binding and serving an already assembled state-complete HTTP service.

use crate::{
    announced_close::{AnnouncedClose, record_close_announcement},
    close_policy::ClosePolicy,
    config::ServerConfig,
    error::HttpServerError,
    listener_security::ListenerSecurity,
    staged_close_listener::StagedCloseListener,
    tls_tcp_listener::TlsTcpListener,
};
use axum::{ServiceExt, extract::Request, middleware::from_fn, response::Response, serve::Listener};
use std::{
    convert::Infallible,
    fmt::Debug,
    future::{Future, IntoFuture},
    net::SocketAddr,
};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};
use tower::{Layer, Service};

/// Bind the configured listener and serve until the supplied shutdown future resolves.
///
/// `security` decides whether the bound socket serves plaintext or terminates
/// TLS; composition derives it from the listener's configured transport.
///
/// # Errors
///
/// Returns [`HttpServerError::Bind`] when the configured address cannot be bound,
/// [`HttpServerError::LocalAddress`] when the bound listener's effective address cannot be read,
/// [`HttpServerError::Serve`] when connection acceptance or serving fails, and
/// [`HttpServerError::DrainDeadlineExceeded`] when accepted connections are still in flight once
/// the configured graceful-drain deadline has passed.
pub async fn serve<S, F>(config: &ServerConfig, security: ListenerSecurity, service: S, shutdown: F) -> Result<(), HttpServerError>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    F: Future<Output = ()> + Send,
{
    let address = SocketAddr::from((config.host, config.port));
    let listener = TcpListener::bind(address).await.map_err(|source| HttpServerError::Bind { address, source })?;
    let local_address = listener.local_addr().map_err(|source| HttpServerError::LocalAddress { source })?;
    tracing::info!(address = %local_address, transport = %security.transport(), "HTTP listener accepting connections");
    let policy = config.close_policy();
    match security {
        ListenerSecurity::Plaintext => serve_listener(listener, service, shutdown, policy).await,
        ListenerSecurity::Tls { feed, handshake_timeout } => {
            serve_listener(TlsTcpListener::new(listener, feed, handshake_timeout), service, shutdown, policy).await
        }
    }
}

/// Serve an already bound listener, allowing composition and tests to control its address.
///
/// A connection on which a response announced `Connection: close` closes in
/// stages, delayed by the policy's announced-close delay (see
/// [`crate::staged_close_stream`]); every other connection closes as soon as
/// it is done.
///
/// # Errors
///
/// Returns [`HttpServerError::Serve`] when connection acceptance or serving fails, and
/// [`HttpServerError::DrainDeadlineExceeded`] when accepted connections are still in flight once
/// the policy's drain deadline has passed.
pub async fn serve_listener<L, S, F>(listener: L, service: S, shutdown: F, policy: ClosePolicy) -> Result<(), HttpServerError>
where
    L: Listener,
    L::Addr: Debug,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    F: Future<Output = ()> + Send,
{
    let (drain_sender, drain_receiver) = oneshot::channel();
    let graceful = async move {
        match drain_receiver.await {
            Ok(()) | Err(_) => {}
        }
    };
    let service = from_fn(record_close_announcement).layer(service);
    let server = axum::serve(
        StagedCloseListener::new(listener, policy.announced_close_delay),
        ServiceExt::<Request>::into_make_service_with_connect_info::<AnnouncedClose>(service),
    )
    .with_graceful_shutdown(graceful)
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result.map_err(|source| HttpServerError::Serve { source }),
        () = shutdown => {
            match drain_sender.send(()) {
                Ok(()) | Err(()) => {}
            }
            match timeout(policy.drain_deadline, &mut server).await {
                Ok(result) => result.map_err(|source| HttpServerError::Serve { source }),
                Err(_) => Err(HttpServerError::DrainDeadlineExceeded { deadline: policy.drain_deadline }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{close_policy::ClosePolicy, error::HttpServerError, execution::serve_listener};
    use axum::{
        Router,
        http::{StatusCode, header::CONNECTION},
        routing::{get, post},
    };
    use std::{io, net::SocketAddr, sync::Arc, time::Duration};
    use tokio::{
        net::TcpListener,
        sync::{Notify, oneshot},
        task::JoinError,
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
        #[error("shutdown receiver was dropped")]
        Shutdown,
    }

    /// The policy every fixture server runs under unless a test says otherwise.
    const POLICY: ClosePolicy = ClosePolicy {
        drain_deadline: Duration::from_secs(2),
        announced_close_delay: Duration::from_millis(500),
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
}
