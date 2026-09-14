//! Binding the QUIC (UDP) endpoint that carries HTTP/3.
//!
//! HTTP/3 shares the TCP listener's port number, not its socket: QUIC is a
//! UDP-based transport, so `UDP:port` and `TCP:port` are independent bindings
//! that coexist without conflict. This module binds only the UDP side; the TCP
//! listener that serves HTTP/1.1 and HTTP/2 is bound elsewhere.

use crate::{
    config::Http3Activation,
    tls::{Http3TlsError, quic_server_crypto},
};
use quinn::{Endpoint, ServerConfig};
use std::{io, net::SocketAddr, sync::Arc};
use thiserror::Error;

/// Failure to bind the QUIC endpoint.
#[derive(Debug, Error)]
pub enum Http3EndpointError {
    /// The TLS configuration could not be assembled.
    #[error(transparent)]
    Tls(#[from] Http3TlsError),
    /// The UDP socket could not be bound.
    #[error("cannot bind QUIC endpoint to {address}: {source}")]
    Bind {
        /// The address the endpoint was to bind.
        address: SocketAddr,
        /// The underlying I/O failure.
        source: io::Error,
    },
}

/// Bind a QUIC server endpoint terminating TLS for the given activation.
///
/// The endpoint listens on `address` over UDP. It must be called from within a
/// Tokio runtime, as quinn drives the socket on the ambient runtime.
///
/// # Errors
///
/// Returns [`Http3EndpointError::Tls`] when the certificate material cannot be
/// turned into a QUIC configuration, and [`Http3EndpointError::Bind`] when the
/// UDP socket cannot be bound.
pub fn bind(activation: Http3Activation<'_>, address: SocketAddr) -> Result<Endpoint, Http3EndpointError> {
    let crypto = quic_server_crypto(activation)?;
    let server_config = ServerConfig::with_crypto(Arc::new(crypto));
    Endpoint::server(server_config, address).map_err(|source| Http3EndpointError::Bind { address, source })
}

#[cfg(test)]
mod tests {
    use crate::{endpoint::bind, test_support::SelfSignedListener};
    use std::{
        error::Error,
        net::{Ipv4Addr, SocketAddr},
    };

    #[tokio::test]
    async fn an_endpoint_binds_an_ephemeral_udp_port() -> Result<(), Box<dyn Error>> {
        let listener = SelfSignedListener::generate()?;
        let activation = listener.config().activation()?.expect("enabled config yields activation");
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let endpoint = bind(activation, address)?;
        let bound = endpoint.local_addr()?;
        assert_ne!(bound.port(), 0, "the endpoint was assigned a concrete port");
        endpoint.close(0u32.into(), b"test complete");
        Ok(())
    }
}
