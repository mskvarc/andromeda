//! An in-memory TLS client that handshakes with a server configuration, so a
//! test can see what the configuration negotiates and presents without binding
//! a socket.

use crate::crypto_provider::broker_crypto_provider;
use rustls::{ClientConfig, ClientConnection, ProtocolVersion, RootCertStore, ServerConfig, ServerConnection, SupportedProtocolVersion};
use rustls_pki_types::{CertificateDer, ServerName};
use std::{error::Error, sync::Arc};

/// What a completed handshake negotiated.
pub(crate) struct HandshakeOutcome {
    /// The TLS version both sides agreed on.
    pub(crate) version: Option<ProtocolVersion>,
    /// The leaf certificate the server presented.
    pub(crate) leaf: Option<CertificateDer<'static>>,
}

/// Complete a handshake between `server` and a client trusting `roots` and
/// offering only `versions`, shuttling records between them in memory.
pub(crate) fn handshake(
    server: Arc<ServerConfig>,
    roots: RootCertStore,
    versions: &[&'static SupportedProtocolVersion],
) -> Result<HandshakeOutcome, Box<dyn Error + Send + Sync>> {
    let client_config = ClientConfig::builder_with_provider(Arc::new(broker_crypto_provider()))
        .with_protocol_versions(versions)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let mut client = ClientConnection::new(Arc::new(client_config), ServerName::try_from("localhost")?)?;
    let mut server = ServerConnection::new(server)?;
    while client.is_handshaking() || server.is_handshaking() {
        let mut records = Vec::new();
        client.write_tls(&mut records)?;
        server.read_tls(&mut records.as_slice())?;
        server.process_new_packets()?;
        records.clear();
        server.write_tls(&mut records)?;
        client.read_tls(&mut records.as_slice())?;
        client.process_new_packets()?;
    }
    Ok(HandshakeOutcome {
        version: server.protocol_version(),
        leaf: client.peer_certificates().and_then(|chain| chain.first()).map(|leaf| leaf.clone().into_owned()),
    })
}
