//! Binding the QUIC (UDP) endpoint that carries HTTP/3.
//!
//! HTTP/3 shares the TCP listener's port number, not its socket: QUIC is a
//! UDP-based transport, so `UDP:port` and `TCP:port` are independent bindings
//! that coexist without conflict. This module binds only the UDP side; the TCP
//! listener that serves HTTP/1.1 and HTTP/2 is bound elsewhere.

use crate::quic_crypto::{QuicCryptoError, quic_server_config};
use andromeda_server_tls::server_config_feed::ServerConfigFeed;
use quinn::Endpoint;
use std::{io, net::SocketAddr};
use thiserror::Error;

/// Failure to bind the QUIC endpoint.
#[derive(Debug, Error)]
pub enum Http3EndpointError {
    /// The TLS configuration could not be turned into a QUIC configuration.
    #[error(transparent)]
    Crypto(#[from] QuicCryptoError),
    /// The UDP socket could not be bound.
    #[error("cannot bind QUIC endpoint to {address}: {source}")]
    Bind {
        /// The address the endpoint was to bind.
        address: SocketAddr,
        /// The underlying I/O failure.
        source: io::Error,
    },
}

/// Bind a QUIC server endpoint terminating TLS with the feed's current
/// configuration.
///
/// The endpoint listens on `address` over UDP. It must be called from within a
/// Tokio runtime, as quinn drives the socket on the ambient runtime. Later
/// configurations from the feed are applied while serving, by
/// [`serve_endpoint`](crate::execution::serve_endpoint).
///
/// # Errors
///
/// Returns [`Http3EndpointError::Crypto`] when the configuration is not
/// QUIC-compatible, and [`Http3EndpointError::Bind`] when the UDP socket cannot
/// be bound.
pub fn bind(feed: &ServerConfigFeed, address: SocketAddr) -> Result<Endpoint, Http3EndpointError> {
    let server_config = quic_server_config(feed.current())?;
    Endpoint::server(server_config, address).map_err(|source| Http3EndpointError::Bind { address, source })
}

#[cfg(test)]
mod tests {
    use crate::{endpoint::bind, test_support::QuicIdentity};
    use std::{
        error::Error,
        net::{Ipv4Addr, SocketAddr},
    };

    #[tokio::test]
    async fn an_endpoint_binds_an_ephemeral_udp_port() -> Result<(), Box<dyn Error + Send + Sync>> {
        let identity = QuicIdentity::anonymous()?;
        let endpoint = bind(identity.feed(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let bound = endpoint.local_addr()?;
        assert_ne!(bound.port(), 0, "the endpoint was assigned a concrete port");
        endpoint.close(0u32.into(), b"test complete");
        Ok(())
    }
}
