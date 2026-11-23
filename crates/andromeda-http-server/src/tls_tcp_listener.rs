//! A TCP listener that terminates TLS before handing connections to axum.
//!
//! Handshakes run concurrently and under a deadline, so a client that connects
//! and then stalls, or never speaks TLS at all, cannot hold up the accept
//! loop for anyone else. A failed handshake is logged at `debug`: every
//! Kubernetes `tcpSocket` probe, load-balancer health check and port scan
//! opens a connection and closes it without a handshake, and logging each at
//! `error` would bury real failures.

use crate::{accept_backoff::back_off_after, tls_acceptor::FeedTlsAcceptor};
use andromeda_server_tls::server_config_feed::ServerConfigFeed;
use axum::serve::Listener;
use std::{io, net::SocketAddr, time::Duration};
use tls_listener::{Error as TlsListenerError, TlsListener, builder};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;

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

#[cfg(test)]
mod tests;
