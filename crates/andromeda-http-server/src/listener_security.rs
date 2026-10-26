//! What a TCP listener needs to know to speak its configured transport.

use andromeda_configuration::schema::listener_transport::ListenerTransport;
use andromeda_server_tls::{
    handshake_policy::{HandshakePolicy, ProtocolFloor},
    server_config_feed::ServerConfigFeed,
};
use std::time::Duration;

/// The handshake a TCP listener asks for: HTTP/2 preferred over HTTP/1.1
/// (RFC 7301 ALPN tokens `h2` and `http/1.1`), under the configured TLS floor.
///
/// Both protocols are served on one socket; the ALPN outcome only tells the
/// connection which one the client chose.
pub const TCP_HANDSHAKE_POLICY: HandshakePolicy = HandshakePolicy {
    alpn: &[b"h2", b"http/1.1"],
    protocol_floor: ProtocolFloor::Configured,
};

/// How a TCP listener protects its connections.
#[derive(Debug, Clone)]
pub enum ListenerSecurity {
    /// Plaintext HTTP, for deployment behind a TLS terminator.
    Plaintext,
    /// TLS terminated in process.
    Tls {
        /// The live server configuration each handshake reads.
        feed: ServerConfigFeed,
        /// How long a client may take to complete its handshake.
        handshake_timeout: Duration,
    },
}

impl ListenerSecurity {
    /// The transport this security speaks, as configuration names it.
    #[must_use]
    pub const fn transport(&self) -> ListenerTransport {
        match self {
            ListenerSecurity::Plaintext => ListenerTransport::Plaintext,
            ListenerSecurity::Tls { .. } => ListenerTransport::Tls,
        }
    }
}
