//! Serving the broker's HTTP listeners under one shutdown signal.
//!
//! The broker exposes the same NGSI-LD API over two transports that share a
//! port number: a TCP listener carrying HTTP/1.1 and HTTP/2, and, when enabled,
//! a QUIC listener carrying HTTP/3. Both are driven from the single process
//! shutdown future here, and while HTTP/3 is served the TCP responses carry an
//! `Alt-Svc` header advertising the QUIC endpoint so a client that first arrives
//! over TCP can upgrade. The two listeners run concurrently; either one failing
//! cancels the other so the process never keeps serving on just one transport.

use crate::config::runtime::RuntimeConfig;
use andromeda_http_api::api::HttpApi;
use andromeda_http_headers::alt_svc::{AltSvcAdvertisement, InvalidAdvertisement};
use andromeda_http_server::{error::HttpServerError, execution::serve as serve_tcp};
use andromeda_http3_server::{
    config::Http3ConfigError,
    endpoint::{self, Http3EndpointError},
    execution::{Http3ServeError, serve_endpoint},
};
use std::{future::Future, net::SocketAddr, num::NonZeroU16, time::Duration};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tower::{Layer, util::option_layer};
use tower_http::set_header::SetResponseHeaderLayer;

/// Failure to serve one of the broker's HTTP listeners.
#[derive(Debug, Error)]
pub enum ListenerError {
    /// The TCP (HTTP/1.1 and HTTP/2) listener failed.
    #[error(transparent)]
    Tcp(#[from] HttpServerError),
    /// The HTTP/3 listener configuration was invalid.
    #[error(transparent)]
    Http3Config(#[from] Http3ConfigError),
    /// The HTTP/3 (QUIC) endpoint could not be bound.
    #[error(transparent)]
    Http3Bind(#[from] Http3EndpointError),
    /// The HTTP/3 (QUIC) listener failed while serving.
    #[error(transparent)]
    Http3Serve(#[from] Http3ServeError),
    /// The HTTP/3 `Alt-Svc` advertisement could not be built.
    #[error(transparent)]
    Advertisement(#[from] InvalidAdvertisement),
    /// The listener port was zero, which cannot be advertised for HTTP/3.
    #[error("the listener port must be non-zero to advertise HTTP/3")]
    ZeroPort,
}

/// Serve the broker's HTTP listeners until `shutdown` resolves.
///
/// The TCP listener is always served. When [`RuntimeConfig::http3`] is enabled,
/// a QUIC listener is bound on the same host and port over UDP and served
/// concurrently, and the TCP responses advertise it through `Alt-Svc`.
///
/// # Errors
///
/// Returns a [`ListenerError`] when either listener cannot be bound or fails
/// while serving, or when the HTTP/3 advertisement cannot be built.
pub async fn serve(config: &RuntimeConfig, api: HttpApi, shutdown: impl Future<Output = ()> + Send + 'static) -> Result<(), ListenerError> {
    let address = SocketAddr::from((config.server.host, config.server.port));
    // Validated identically at configuration load; re-read here to bind with.
    let activation = config.http3.activation()?;

    // The HTTP/3 advertisement and router are prepared before the API is
    // consumed into the TCP service, because axum's HTTP/3 adapter drives a
    // bare router rather than the process-stacked TCP service.
    let advertisement = match activation {
        Some(_) => {
            let port = NonZeroU16::new(config.server.port).ok_or(ListenerError::ZeroPort)?;
            Some(AltSvcAdvertisement::h3(port, Duration::from_secs(config.http3.advertisement_max_age_secs))?)
        }
        None => None,
    };
    let h3_router = activation.is_some().then(|| api.h3_router());

    // Advertise HTTP/3 on TCP responses only while the QUIC listener is up; the
    // layer is absent, not merely empty, when HTTP/3 is disabled.
    let alt_svc_layer =
        advertisement.map(|advertisement| SetResponseHeaderLayer::if_not_present(AltSvcAdvertisement::name().clone(), advertisement.value().clone()));
    let tcp_service = option_layer(alt_svc_layer).layer(api.into_service());

    let shutdown_token = CancellationToken::new();
    let signal_token = shutdown_token.clone();
    let signal = tokio::spawn(async move {
        shutdown.await;
        signal_token.cancel();
    });

    let tcp = serve_tcp(&config.server, tcp_service, wait_for(&shutdown_token));

    let result = match (activation, h3_router) {
        (Some(activation), Some(router)) => {
            let endpoint = endpoint::bind(activation, address)?;
            let http3 = serve_endpoint(endpoint, router, wait_for(&shutdown_token));
            let tcp = async { tcp.await.map_err(ListenerError::from) };
            let http3 = async { http3.await.map_err(ListenerError::from) };
            tokio::try_join!(tcp, http3).map(|((), ())| ())
        }
        _ => tcp.await.map_err(ListenerError::from),
    };

    // If a listener returned an error before the shutdown signal fired, cancel
    // so the spawned signal task can complete rather than leak.
    shutdown_token.cancel();
    if signal.await.is_err() {
        tracing::warn!(event = "shutdown_signal_task_panicked");
    }
    result
}

/// A future that resolves when `token` is cancelled, owning a clone of it so it
/// outlives the borrow of the caller's token.
fn wait_for(token: &CancellationToken) -> impl Future<Output = ()> + Send + 'static {
    let token = token.clone();
    async move { token.cancelled().await }
}
