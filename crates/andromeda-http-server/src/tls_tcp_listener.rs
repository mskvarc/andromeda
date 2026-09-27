//! A TCP listener that terminates TLS before handing connections to axum.
//!
//! Handshakes run concurrently and under a deadline, so a client that connects
//! and then stalls -- or never speaks TLS at all -- cannot hold up the accept
//! loop for anyone else. A failed handshake is logged at `debug`: every
//! Kubernetes `tcpSocket` probe, load-balancer health check and port scan
//! opens a connection and closes it without a handshake, and logging each at
//! `error` would bury real failures.

use crate::tls_acceptor::FeedTlsAcceptor;
use andromeda_server_tls::server_config_feed::ServerConfigFeed;
use axum::serve::Listener;
use std::{io, net::SocketAddr, time::Duration};
use tls_listener::{Error as TlsListenerError, TlsListener, builder};
use tokio::{
    net::{TcpListener, TcpStream},
    time::sleep,
};
use tokio_rustls::server::TlsStream;

/// How long the accept loop backs off after a listener-level failure, such as
/// running out of file descriptors, before trying again.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_secs(1);

/// A bound TCP socket whose accepted connections have completed a TLS handshake.
pub(crate) struct TlsTcpListener {
    /// The socket and the handshakes in progress on it.
    inner: TlsListener<TcpListener, FeedTlsAcceptor>,
}

impl TlsTcpListener {
    /// Terminate TLS on `listener` with the live configuration in `feed`,
    /// dropping any connection whose handshake outlasts `handshake_timeout`.
    pub(crate) fn new(listener: TcpListener, feed: ServerConfigFeed, handshake_timeout: Duration) -> TlsTcpListener {
        TlsTcpListener {
            inner: builder(FeedTlsAcceptor(feed)).handshake_timeout(handshake_timeout).listen(listener),
        }
    }
}

impl Listener for TlsTcpListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.inner.accept().await {
                Ok(connection) => return connection,
                Err(TlsListenerError::ListenerError(error)) => back_off_after(error).await,
                Err(handshake) => tracing::debug!(event = "tls_handshake_failed", peer = ?handshake.peer_addr(), error = %handshake),
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// Treat a listener-level accept failure the way axum's own TCP listener does.
///
/// A connection that was reset or aborted before it could be accepted is the
/// peer's business and is skipped. Anything else -- typically `EMFILE` -- is
/// logged and waited out, in the hope that closing connections frees what the
/// listener needs.
async fn back_off_after(error: io::Error) {
    // `io::ErrorKind` is non-exhaustive, so the skipped kinds are tested for
    // rather than matched exhaustively.
    if matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset
    ) {
        return;
    }
    tracing::error!(event = "tcp_accept_failed", error = %error, "backing off before accepting again");
    sleep(ACCEPT_ERROR_BACKOFF).await;
}

#[cfg(test)]
mod tests;
