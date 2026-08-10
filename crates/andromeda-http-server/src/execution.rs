//! Binding and serving an already assembled state-complete HTTP service.

use crate::{config::ServerConfig, error::HttpServerError};
use axum::{ServiceExt, extract::Request, response::Response};
use std::{
    convert::Infallible,
    future::{Future, IntoFuture},
    net::SocketAddr,
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};
use tower::Service;

/// Bind the configured listener and serve until the supplied shutdown future resolves.
pub async fn serve<S, F>(config: &ServerConfig, service: S, shutdown: F) -> Result<(), HttpServerError>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
    F: Future<Output = ()> + Send,
{
    let address = SocketAddr::from((config.host, config.port));
    let listener = TcpListener::bind(address).await.map_err(|source| HttpServerError::Bind { address, source })?;
    let local_address = listener.local_addr().map_err(|source| HttpServerError::LocalAddress { source })?;
    tracing::info!(address = %local_address, "HTTP listener accepting connections");
    serve_listener(listener, service, shutdown, config.graceful_drain_deadline()).await
}

/// Serve an already bound listener, allowing composition and tests to control its address.
pub async fn serve_listener<S, F>(listener: TcpListener, service: S, shutdown: F, drain_deadline: Duration) -> Result<(), HttpServerError>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
    F: Future<Output = ()> + Send,
{
    let (drain_sender, drain_receiver) = oneshot::channel();
    let graceful = async move {
        match drain_receiver.await {
            Ok(()) | Err(_) => {}
        }
    };
    let server = axum::serve(listener, ServiceExt::<Request>::into_make_service(service))
        .with_graceful_shutdown(graceful)
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result.map_err(|source| HttpServerError::Serve { source }),
        () = shutdown => {
            match drain_sender.send(()) {
                Ok(()) | Err(()) => {}
            }
            match timeout(drain_deadline, &mut server).await {
                Ok(result) => result.map_err(|source| HttpServerError::Serve { source }),
                Err(_) => Err(HttpServerError::DrainDeadlineExceeded { deadline: drain_deadline }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::HttpServerError, execution::serve_listener};
    use axum::{Router, routing::get};
    use std::{io, net::SocketAddr, time::Duration};
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

    async fn ephemeral_listener() -> Result<(TcpListener, SocketAddr), io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        Ok((listener, address))
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
            Duration::from_secs(1),
        ));
        let response = reqwest::get(format!("http://{address}/test")).await?;
        pretty_assertions::assert_eq!(response.text().await?, "served");
        shutdown_sender.send(()).map_err(|_| TestError::Shutdown)?;
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn graceful_shutdown_waits_for_an_in_flight_request_to_complete() -> Result<(), TestError> {
        let (listener, address) = ephemeral_listener().await?;
        let started = std::sync::Arc::new(Notify::new());
        let release = std::sync::Arc::new(Notify::new());
        let handler_started = std::sync::Arc::clone(&started);
        let handler_release = std::sync::Arc::clone(&release);
        let router = Router::new().route(
            "/slow",
            get(move || {
                let started = std::sync::Arc::clone(&handler_started);
                let release = std::sync::Arc::clone(&handler_release);
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
            Duration::from_secs(2),
        ));
        let request = tokio::spawn(reqwest::get(format!("http://{address}/slow")));
        started.notified().await;
        shutdown_sender.send(()).map_err(|_| TestError::Shutdown)?;
        release.notify_one();
        let response = request.await??;
        pretty_assertions::assert_eq!(response.text().await?, "complete");
        server.await??;
        Ok(())
    }
}
