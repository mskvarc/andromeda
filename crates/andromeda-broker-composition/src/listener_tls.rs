//! How the broker's listeners terminate TLS, and the scheme each one mints.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2 requires the API to support HTTP over TLS.
//! Every listener that terminates it -- the data-plane TCP listener when
//! `server.transport = "tls"`, the HTTP/3 listener whenever it is enabled, and
//! the `separate-port` management listener when `management.transport = "tls"`
//! -- presents the one `[tls]` identity, reloaded from disk as it rotates. The
//! identity is loaded only if some listener asks for it, so a plaintext broker
//! behind a reverse proxy never reads a certificate.

use andromeda_configuration::schema::{listener_transport::ListenerTransport, tls::TlsConfig};
use andromeda_http_headers::{api_root_scheme::ApiRootScheme, listener_scheme::ListenerScheme};
use andromeda_http_server::listener_security::{ListenerSecurity, TCP_HANDSHAKE_POLICY};
use andromeda_http3_server::quic_crypto::QUIC_HANDSHAKE_POLICY;
use andromeda_server_tls::{
    handshake_policy::HandshakeConfigError,
    material_watch::{TlsMaterialWatch, TlsWatchError},
    server_config_feed::ServerConfigFeed,
};
use thiserror::Error;

/// The shared identity could not be loaded or turned into a listener's configuration.
#[derive(Debug, Error)]
pub enum ListenerTlsError {
    /// The `[tls]` identity could not be loaded or watched.
    #[error(transparent)]
    Identity(#[from] TlsWatchError),
    /// A listener's TLS configuration could not be built from the identity.
    #[error(transparent)]
    Handshake(#[from] HandshakeConfigError),
}

/// The `[tls]` identity the listeners share, loaded the first time a listener
/// asks for it and watched for as long as this value lives.
pub struct SharedTlsIdentity<'config> {
    /// The identity section.
    config: &'config TlsConfig,
    /// The loaded, watched material, once some listener has asked for it.
    watch: Option<TlsMaterialWatch>,
}

impl<'config> SharedTlsIdentity<'config> {
    /// An identity that loads `config` on first use.
    #[must_use]
    pub const fn new(config: &'config TlsConfig) -> SharedTlsIdentity<'config> {
        SharedTlsIdentity { config, watch: None }
    }

    /// The security a TCP listener with `transport` serves under.
    ///
    /// # Errors
    ///
    /// Returns a [`ListenerTlsError`] when `transport` is TLS and the identity
    /// cannot be loaded or configured.
    pub fn tcp_security(&mut self, transport: ListenerTransport) -> Result<ListenerSecurity, ListenerTlsError> {
        match transport {
            ListenerTransport::Plaintext => Ok(ListenerSecurity::Plaintext),
            ListenerTransport::Tls => Ok(ListenerSecurity::Tls {
                feed: self.watch()?.feed(TCP_HANDSHAKE_POLICY)?,
                handshake_timeout: self.config.handshake_timeout.duration(),
            }),
        }
    }

    /// The live configuration the QUIC listener serves.
    ///
    /// # Errors
    ///
    /// Returns a [`ListenerTlsError`] when the identity cannot be loaded or
    /// configured for QUIC.
    pub fn quic_feed(&mut self) -> Result<ServerConfigFeed, ListenerTlsError> {
        Ok(self.watch()?.feed(QUIC_HANDSHAKE_POLICY)?)
    }

    /// The loaded identity, loading and watching it on first use.
    fn watch(&mut self) -> Result<&TlsMaterialWatch, TlsWatchError> {
        let watch = match self.watch.take() {
            Some(watch) => watch,
            None => TlsMaterialWatch::start(self.config)?,
        };
        Ok(self.watch.insert(watch))
    }
}

/// The scheme a listener speaking `transport` mints its URLs with, installed
/// on its service as a request extension.
#[must_use]
pub const fn listener_scheme(transport: ListenerTransport) -> ListenerScheme {
    match transport {
        ListenerTransport::Plaintext => ListenerScheme::new(ApiRootScheme::Http),
        ListenerTransport::Tls => ListenerScheme::new(ApiRootScheme::Https),
    }
}

#[cfg(test)]
mod tests {
    use crate::listener_tls::{SharedTlsIdentity, listener_scheme};
    use andromeda_configuration::schema::{listener_transport::ListenerTransport, tls::TlsConfig};
    use andromeda_http_headers::{api_root_scheme::ApiRootScheme, listener_scheme::ListenerScheme};
    use andromeda_http_server::listener_security::ListenerSecurity;
    use http::{Request, Response};
    use std::{convert::Infallible, error::Error};
    use tower::{Layer, ServiceExt, service_fn};
    use tower_http::add_extension::AddExtensionLayer;

    /// The scheme a request sees after passing the layer for `transport`.
    async fn scheme_seen_through(transport: ListenerTransport) -> Result<Option<ListenerScheme>, Infallible> {
        let service = AddExtensionLayer::new(listener_scheme(transport)).layer(service_fn(|request: Request<()>| async move {
            Ok::<_, Infallible>(Response::new(request.extensions().get::<ListenerScheme>().copied()))
        }));
        Ok(service.oneshot(Request::new(())).await?.into_body())
    }

    #[tokio::test]
    async fn the_tcp_service_carries_the_listener_scheme_of_its_transport() -> Result<(), Infallible> {
        pretty_assertions::assert_eq!(
            scheme_seen_through(ListenerTransport::Tls).await?.map(ApiRootScheme::from),
            Some(ApiRootScheme::Https)
        );
        pretty_assertions::assert_eq!(
            scheme_seen_through(ListenerTransport::Plaintext).await?.map(ApiRootScheme::from),
            Some(ApiRootScheme::Http)
        );
        Ok(())
    }

    /// A plaintext listener never loads the identity, so an empty `[tls]`
    /// section is no obstacle to it.
    #[test]
    fn a_plaintext_listener_needs_no_identity() -> Result<(), Box<dyn Error>> {
        let config = TlsConfig::default();
        let mut identity = SharedTlsIdentity::new(&config);
        assert!(matches!(identity.tcp_security(ListenerTransport::Plaintext)?, ListenerSecurity::Plaintext));
        Ok(())
    }

    #[test]
    fn a_tls_listener_without_an_identity_fails_to_start() {
        let config = TlsConfig::default();
        let mut identity = SharedTlsIdentity::new(&config);
        assert!(identity.tcp_security(ListenerTransport::Tls).is_err());
    }
}
