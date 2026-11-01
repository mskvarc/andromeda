//! A bare QUIC handshake with a broker's HTTP/3 listener, offering the application protocols a test chooses, for a
//! test of what the listener admits before any HTTP/3 exchange (IETF RFC 9114 section 3.1, IETF RFC 9001 section
//! 8.1). An HTTP/3 client always offers `h3`, so a handshake offering anything else needs a QUIC client of its own.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    connection_outcome::ConnectionOutcome,
    tls_connection::{TlsError, read, socket_address},
};
use quinn::{
    ClientConfig,
    ConnectError,
    Endpoint,
    crypto::rustls::{NoInitialCipherSuite, QuicClientConfig},
};
use rustls::{
    RootCertStore,
    crypto::aws_lc_rs,
    pki_types::{
        CertificateDer,
        pem::{self, PemObject},
    },
    version::TLS13,
};
use std::{
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

/// An application protocol a QUIC client offers in ALPN (IETF RFC 7301).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationProtocol {
    /// HTTP/3, `h3` (IETF RFC 9114 section 3.1).
    Http3,
    /// HTTP/2 over TLS, `h2` (IETF RFC 9113 section 3.2), which QUIC does not carry.
    Http2,
}

impl ApplicationProtocol {
    /// The protocol's ALPN identification sequence.
    const fn identification(self) -> &'static [u8] {
        match self {
            ApplicationProtocol::Http3 => b"h3",
            ApplicationProtocol::Http2 => b"h2",
        }
    }
}

/// Why a QUIC handshake could not be attempted.
#[derive(Debug, thiserror::Error)]
pub enum QuicHandshakeError {
    /// The stack does not name the broker or its directory.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The broker's address or the authority file cannot be had.
    #[error(transparent)]
    Tls(#[from] TlsError),
    /// The broker URL names no host to verify the certificate against.
    #[error("the broker URL of {0:?} names no host")]
    NoHost(BrokerRole),
    /// The authority file holds no PEM certificate.
    #[error("{0} holds no PEM certificate")]
    Pem(PathBuf, #[source] pem::Error),
    /// The authority's certificate cannot be trusted.
    #[error("{0} cannot be trusted")]
    Authority(PathBuf, #[source] rustls::Error),
    /// The client TLS configuration cannot be built.
    #[error("the client TLS configuration cannot be built")]
    Configuration(#[source] rustls::Error),
    /// The client TLS configuration offers no cipher suite QUIC can start with.
    #[error("the client TLS configuration cannot carry QUIC")]
    Cipher(#[source] NoInitialCipherSuite),
    /// The client's UDP socket cannot be bound.
    #[error("the client QUIC endpoint cannot be bound")]
    Bind(#[source] io::Error),
    /// The handshake cannot be started.
    #[error("the QUIC handshake cannot be started")]
    Connect(#[source] ConnectError),
}

/// What becomes of a QUIC handshake with the broker of `role`, trusting only the authority in the PEM file
/// `authority` beneath the stack's directory and offering `protocols`. A completed handshake is closed again at once.
///
/// # Errors
///
/// Returns [`QuicHandshakeError`] when the handshake cannot be attempted.
pub async fn quic_handshake(role: BrokerRole, authority: &str, protocols: &[ApplicationProtocol]) -> Result<ConnectionOutcome, QuicHandshakeError> {
    let config = SuiteConfig::get().map_err(|error| QuicHandshakeError::Config(error.clone()))?;
    let broker = config.broker(role)?;
    let server_name = broker.host_str().ok_or(QuicHandshakeError::NoHost(role))?;
    let address = socket_address(config, broker).await?;
    let client = client_config(&config.stack_dir()?.join(authority), protocols)?;
    let unspecified = match address {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
    };
    let endpoint = Endpoint::client(unspecified).map_err(QuicHandshakeError::Bind)?;
    let result = endpoint.connect_with(client, address, server_name).map_err(QuicHandshakeError::Connect)?.await;
    let outcome = ConnectionOutcome::of_quic_handshake(&result);
    if let Ok(connection) = result {
        connection.close(0_u32.into(), b"");
    }
    endpoint.wait_idle().await;
    Ok(outcome)
}

/// A QUIC client configuration trusting only the certificate in the PEM file `authority` and offering `protocols`.
fn client_config(authority: &Path, protocols: &[ApplicationProtocol]) -> Result<ClientConfig, QuicHandshakeError> {
    let certificate = CertificateDer::from_pem_slice(&read(authority)?).map_err(|error| QuicHandshakeError::Pem(authority.to_owned(), error))?;
    let mut roots = RootCertStore::empty();
    roots
        .add(certificate)
        .map_err(|error| QuicHandshakeError::Authority(authority.to_owned(), error))?;
    let mut crypto = rustls::ClientConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
        .with_protocol_versions(&[&TLS13])
        .map_err(QuicHandshakeError::Configuration)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = protocols.iter().map(|protocol| protocol.identification().to_vec()).collect();
    let crypto = crypto;
    let quic = QuicClientConfig::try_from(crypto).map_err(QuicHandshakeError::Cipher)?;
    Ok(ClientConfig::new(Arc::new(quic)))
}

#[cfg(test)]
mod tests {
    use crate::quic_handshake::ApplicationProtocol;
    use googletest::prelude::*;

    #[gtest]
    fn each_protocol_is_offered_under_its_registered_identification() {
        expect_that!(ApplicationProtocol::Http3.identification(), eq(b"h3"));
        expect_that!(ApplicationProtocol::Http2.identification(), eq(b"h2"));
    }
}
