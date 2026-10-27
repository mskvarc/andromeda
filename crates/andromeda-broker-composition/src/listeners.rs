//! Serving the broker's HTTP listeners under one shutdown signal.
//!
//! The broker exposes the same NGSI-LD API over two transports that share a
//! port number: a TCP listener carrying HTTP/1.1 and HTTP/2, and, when enabled,
//! a QUIC listener carrying HTTP/3. Both are driven from the single process
//! shutdown future here, and while HTTP/3 is served the TCP responses carry an
//! `Alt-Svc` header advertising the QUIC endpoint so a client that first arrives
//! over TCP can upgrade. In `separate-port` management mode a third, TCP-only
//! listener carries the observability endpoints on their own host and port, with
//! no QUIC and no `Alt-Svc`. Every listener runs concurrently under one shutdown
//! token; the first to fail cancels the rest so the process never keeps serving
//! on a subset of them.
//!
//! Each listener tells the requests it carries which scheme it speaks, so the
//! URLs the broker mints from a request origin carry `https` exactly when the
//! request arrived over TLS the broker terminated (see [`crate::listener_tls`]).

use crate::listener_tls::{ListenerTlsError, SharedTlsIdentity, listener_scheme};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    header_section_limit::HeaderSectionLimit,
    schema::{http3::Http3Mode, server::ServerConfig},
};
use andromeda_http_api::{api::HttpApi, management::ManagementApi};
use andromeda_http_headers::{
    alt_svc::{AltSvcAdvertisement, InvalidAdvertisement},
    api_root_scheme::ApiRootScheme,
    listener_scheme::ListenerScheme,
};
use andromeda_http_server::{close_policy::ClosePolicy, error::HttpServerError, execution::serve as serve_tcp, listener_security::ListenerSecurity};
use andromeda_http3_server::{
    endpoint::{self, Http3EndpointError},
    execution::{Http3ServeError, serve_endpoint},
};
use futures::future::try_join_all;
use std::{future::Future, net::SocketAddr, num::NonZeroU16, pin::Pin};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tower::{Layer, util::option_layer};
use tower_http::{add_extension::AddExtensionLayer, set_header::SetResponseHeaderLayer};

/// A boxed, error-unified listener future driven under the shared shutdown token.
type ListenerFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ListenerError>> + Send + 'a>>;

/// Failure to serve one of the broker's HTTP listeners.
#[derive(Debug, Error)]
pub enum ListenerError {
    /// The TCP (HTTP/1.1 and HTTP/2) listener failed.
    #[error(transparent)]
    Tcp(#[from] HttpServerError),
    /// The shared TLS identity could not be loaded or configured.
    #[error(transparent)]
    Tls(#[from] ListenerTlsError),
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
/// The data-plane TCP listener is always served, in plaintext or over TLS as
/// `server.transport` says. When [`BrokerConfig::http3`] is enabled, a QUIC
/// listener is bound on the same host and port over UDP and served
/// concurrently, and the TCP responses advertise it through `Alt-Svc`. When
/// `management` is `Some` (the `separate-port` mode), a third TCP-only listener
/// serves the observability endpoints on the management host and port, over
/// the transport `management.transport` says. Every TLS-terminating listener
/// presents the shared `[tls]` identity, reloaded as its files rotate.
///
/// # Errors
///
/// Returns a [`ListenerError`] when the shared TLS identity cannot be loaded,
/// when any listener cannot be bound or fails while serving, or when the
/// HTTP/3 advertisement cannot be built.
pub async fn serve(
    config: &BrokerConfig,
    api: HttpApi,
    management: Option<ManagementApi>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ListenerError> {
    let address = SocketAddr::from((config.server.host, config.server.port));
    // Held until every listener has stopped: dropping it stops reloading the
    // certificate files, and each listener keeps the last configuration it had.
    let mut identity = SharedTlsIdentity::new(&config.tls);
    let tcp_security = identity.tcp_security(config.server.transport)?;
    let quic_feed = match config.server.http3.mode {
        Http3Mode::Disabled => None,
        Http3Mode::Enabled => Some(identity.quic_feed()?),
    };
    let management_security = management.as_ref().map(|_| identity.tcp_security(config.management.transport)).transpose()?;

    // The HTTP/3 advertisement and router are prepared before the API is
    // consumed into the TCP service, because axum's HTTP/3 adapter drives a
    // bare router rather than the process-stacked TCP service. QUIC is always
    // TLS, so the HTTP/3 router mints `https` URLs.
    let advertisement = match quic_feed {
        Some(_) => {
            let port = NonZeroU16::new(config.server.port).ok_or(ListenerError::ZeroPort)?;
            Some(AltSvcAdvertisement::h3(port, config.server.http3.advertisement_lifetime.duration())?)
        }
        None => None,
    };
    let h3_router = quic_feed
        .is_some()
        .then(|| api.h3_router().layer(AddExtensionLayer::new(ListenerScheme::new(ApiRootScheme::Https))));

    // Advertise HTTP/3 on TCP responses only while the QUIC listener is up; the
    // layer is absent, not merely empty, when HTTP/3 is disabled.
    let alt_svc_layer =
        advertisement.map(|advertisement| SetResponseHeaderLayer::if_not_present(AltSvcAdvertisement::name().clone(), advertisement.value().clone()));
    let tcp_service = AddExtensionLayer::new(listener_scheme(config.server.transport)).layer(option_layer(alt_svc_layer).layer(api.into_service()));

    let shutdown_token = CancellationToken::new();
    let signal_token = shutdown_token.clone();
    let signal = tokio::spawn(async move {
        shutdown.await;
        signal_token.cancel();
    });

    let mut listeners: Vec<ListenerFuture<'_>> = Vec::new();
    let max_header_section = config.request_limits.max_header_section;
    let tcp = serve_tcp(&config.server, max_header_section, tcp_security, tcp_service, wait_for(&shutdown_token));
    listeners.push(Box::pin(async move { tcp.await.map_err(ListenerError::from) }));

    if let (Some(feed), Some(router)) = (quic_feed, h3_router) {
        let endpoint = endpoint::bind(&feed, address)?;
        let http3 = serve_endpoint(
            endpoint,
            feed,
            router,
            max_header_section,
            ClosePolicy::from(&config.server),
            wait_for(&shutdown_token),
        );
        listeners.push(Box::pin(async move { http3.await.map_err(ListenerError::from) }));
    }

    if let (Some(management), Some(security)) = (management, management_security) {
        // The management listener is TCP-only and reuses the data plane's
        // graceful-drain deadline, announced-close timeout and request header
        // section limit on its own host and port.
        let management_config = ServerConfig {
            host: config.management.host,
            port: config.management.port,
            transport: config.management.transport,
            ..config.server.clone() // clone: the management listener owns its synthesized configuration
        };
        let management_serve = serve_management(management_config, max_header_section, security, management, wait_for(&shutdown_token));
        listeners.push(Box::pin(async move { management_serve.await.map_err(ListenerError::from) }));
    }

    let result = try_join_all(listeners).await.map(|_outcomes| ());
    drop(identity);

    // If a listener returned an error before the shutdown signal fired, cancel
    // so the spawned signal task can complete rather than leak.
    shutdown_token.cancel();
    if signal.await.is_err() {
        tracing::warn!(event = "shutdown_signal_task_panicked");
    }
    result
}

/// Serve the management plane on its own owned listener configuration.
///
/// A thin wrapper that owns the synthesized [`ServerConfig`] so the returned
/// future borrows nothing from the caller and can be boxed alongside the
/// data-plane listeners under one shutdown token.
async fn serve_management(
    config: ServerConfig,
    max_header_section: HeaderSectionLimit,
    security: ListenerSecurity,
    management: ManagementApi,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), HttpServerError> {
    let service = AddExtensionLayer::new(listener_scheme(config.transport)).layer(management.into_service());
    serve_tcp(&config, max_header_section, security, service, shutdown).await
}

/// A future that resolves when `token` is cancelled, owning a clone of it so it
/// outlives the borrow of the caller's token.
fn wait_for(token: &CancellationToken) -> impl Future<Output = ()> + Send + 'static {
    let token = token.clone();
    async move { token.cancelled().await }
}

#[cfg(test)]
mod strict_transport_security_tests;

#[cfg(test)]
mod tests;
