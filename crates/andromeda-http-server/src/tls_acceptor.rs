//! Completing each TLS handshake with whichever configuration is current.

use andromeda_server_tls::server_config_feed::ServerConfigFeed;
use std::io;
use tls_listener::AsyncTls;
use tokio::net::TcpStream;
use tokio_rustls::{Accept, TlsAcceptor, server::TlsStream};

/// A TLS acceptor that reads the live configuration afresh for every
/// connection, so a rotated certificate is presented to the next handshake
/// while connections already open keep the session they negotiated.
#[derive(Debug, Clone)]
pub(crate) struct FeedTlsAcceptor(pub(crate) ServerConfigFeed);

impl AsyncTls<TcpStream> for FeedTlsAcceptor {
    type Stream = TlsStream<TcpStream>;
    type Error = io::Error;
    type AcceptFuture = Accept<TcpStream>;

    fn accept(&self, stream: TcpStream) -> Accept<TcpStream> {
        TlsAcceptor::from(self.0.current()).accept(stream)
    }
}
