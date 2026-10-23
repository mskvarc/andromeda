//! Failures dialing an HTTP/3 listener or exchanging a request with it.

use h3::error::{ConnectionError, StreamError};
use quinn::crypto::rustls::NoInitialCipherSuite;
use std::{io, string::FromUtf8Error};
use thiserror::Error;

/// A test HTTP/3 connection could not be set up, or one exchange on it failed.
#[derive(Debug, Error)]
pub enum Http3ClientError {
    /// The client's TLS configuration could not be built.
    #[error("cannot build the client TLS configuration: {0}")]
    Tls(#[from] rustls::Error),
    /// The TLS configuration offers no cipher suite QUIC can start with.
    #[error("the client TLS configuration cannot carry QUIC: {0}")]
    QuicCrypto(#[from] NoInitialCipherSuite),
    /// The client UDP socket could not be bound.
    #[error("cannot bind the client QUIC endpoint: {0}")]
    Bind(#[from] io::Error),
    /// The connection attempt could not start.
    #[error("cannot start the QUIC connection: {0}")]
    Connect(#[from] quinn::ConnectError),
    /// The QUIC handshake failed.
    #[error("the QUIC handshake failed: {0}")]
    Handshake(#[from] quinn::ConnectionError),
    /// The HTTP/3 connection could not be set up or failed.
    #[error("the HTTP/3 connection failed: {0}")]
    Connection(#[from] ConnectionError),
    /// A request stream failed, the client refusing to send included.
    #[error("the HTTP/3 request stream failed: {0}")]
    Stream(#[from] StreamError),
    /// The request could not be built.
    #[error("cannot build the request: {0}")]
    Request(#[from] http::Error),
    /// The response body is not UTF-8.
    #[error("the response body is not UTF-8: {0}")]
    Text(#[from] FromUtf8Error),
}
