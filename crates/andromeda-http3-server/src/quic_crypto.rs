//! What the QUIC endpoint asks of the broker's shared TLS identity.
//!
//! QUIC carries TLS 1.3 as its only handshake (RFC 9001); there is no cleartext
//! or TLS 1.2 mode. The endpoint therefore asks the shared identity for a
//! TLS-1.3-only configuration advertising the HTTP/3 ALPN token `h3` (RFC 9114
//! clause 3.1), so a client distinguishes it from an HTTP/2-over-QUIC listener,
//! and hands the finished [`rustls::ServerConfig`] to quinn, which wraps it in
//! its own QUIC crypto configuration.

use andromeda_server_tls::handshake_policy::{HandshakePolicy, ProtocolFloor};
use quinn::{
    ServerConfig,
    crypto::rustls::{NoInitialCipherSuite, QuicServerConfig},
};
use std::sync::Arc;
use thiserror::Error;

/// The ALPN protocol identifier for HTTP/3 over QUIC (RFC 9114 clause 3.1).
pub const H3_ALPN: &[u8] = b"h3";

/// The handshake the QUIC endpoint asks of the shared identity.
pub const QUIC_HANDSHAKE_POLICY: HandshakePolicy = HandshakePolicy {
    alpn: &[H3_ALPN],
    protocol_floor: ProtocolFloor::Tls13Only,
};

/// A rustls configuration quinn cannot serve QUIC with.
#[derive(Debug, Error)]
pub enum QuicCryptoError {
    /// The TLS configuration produced no QUIC-compatible initial cipher suite.
    #[error("TLS configuration is not QUIC-compatible: {0}")]
    QuicIncompatible(#[source] NoInitialCipherSuite),
}

/// Wrap a TLS configuration built under [`QUIC_HANDSHAKE_POLICY`] as a quinn
/// server configuration.
///
/// # Errors
///
/// Returns [`QuicCryptoError::QuicIncompatible`] when the configuration's
/// provider offers no cipher suite QUIC can protect its initial packets with.
pub fn quic_server_config(tls: Arc<rustls::ServerConfig>) -> Result<ServerConfig, QuicCryptoError> {
    let crypto = QuicServerConfig::try_from(tls).map_err(QuicCryptoError::QuicIncompatible)?;
    Ok(ServerConfig::with_crypto(Arc::new(crypto)))
}

#[cfg(test)]
mod tests {
    use crate::{
        quic_crypto::{H3_ALPN, quic_server_config},
        test_support::QuicIdentity,
    };
    use std::error::Error;

    #[test]
    fn the_alpn_token_is_the_registered_http3_identifier() {
        assert_eq!(H3_ALPN, b"h3");
    }

    #[tokio::test]
    async fn the_shared_identity_assembles_a_quic_configuration() -> Result<(), Box<dyn Error + Send + Sync>> {
        let identity = QuicIdentity::anonymous()?;
        let tls = identity.feed().current();
        pretty_assertions::assert_eq!(tls.alpn_protocols, vec![H3_ALPN.to_vec()]);
        quic_server_config(tls)?;
        Ok(())
    }
}
