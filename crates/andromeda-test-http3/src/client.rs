//! An HTTP/3 client connection and the responses it collects.

use crate::error::Http3ClientError;
use andromeda_test_tls::identity::TestIdentity;
use bytes::{Buf, Bytes};
use h3::client::{self, SendRequest};
use h3_quinn::{Connection, OpenStreams};
use http::{HeaderMap, Method, Request, StatusCode};
use quinn::{ClientConfig, Endpoint, crypto::rustls::QuicClientConfig};
use rustls::{RootCertStore, crypto::aws_lc_rs::default_provider};
use rustls_pki_types::CertificateDer;
use std::{
    future::poll_fn,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::task::JoinHandle;

/// A response collected in full from an HTTP/3 request stream.
pub struct Http3Response {
    /// The response status.
    pub status: StatusCode,
    /// The response headers.
    pub headers: HeaderMap,
    /// The response body, concatenated across QUIC data frames.
    pub body: Bytes,
}

impl Http3Response {
    /// The response body decoded as UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns [`Http3ClientError::Text`] when the body is not UTF-8.
    pub fn text(&self) -> Result<String, Http3ClientError> {
        Ok(String::from_utf8(self.body.to_vec())?)
    }
}

/// An open HTTP/3 client connection, reusable for several requests.
pub struct Http3Client {
    /// The leaf certificate the server presented.
    server_leaf: Option<CertificateDer<'static>>,
    /// The client-side QUIC endpoint, kept alive for the connection's lifetime.
    endpoint: Endpoint,
    /// The request opener, cloneable to multiplex concurrent request streams.
    send_request: SendRequest<OpenStreams, Bytes>,
    /// The connection driver task, awaited on close.
    driver: JoinHandle<()>,
}

impl Http3Client {
    /// Connect to `server` as `localhost`, verifying its certificate against
    /// `roots`, offering the given ALPN protocols and presenting `identity`,
    /// if any.
    ///
    /// Must be called from within a Tokio runtime, which drives the
    /// connection.
    ///
    /// # Errors
    ///
    /// Returns an [`Http3ClientError`] when the client configuration cannot be
    /// built, the socket cannot be bound, or the QUIC or HTTP/3 handshake
    /// fails.
    pub async fn connect(
        roots: RootCertStore,
        alpn: Vec<Vec<u8>>,
        server: SocketAddr,
        identity: Option<&TestIdentity>,
    ) -> Result<Http3Client, Http3ClientError> {
        let builder = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots);
        let mut crypto = match identity {
            Some(identity) => builder.with_client_auth_cert(vec![identity.certificate().clone()], identity.private_key())?,
            None => builder.with_no_client_auth(),
        };
        crypto.alpn_protocols = alpn;
        let client_config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));
        let mut endpoint = Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        endpoint.set_default_client_config(client_config);
        let connection = endpoint.connect(server, "localhost")?.await?;
        let server_leaf = connection
            .peer_identity()
            .and_then(|identity| identity.downcast::<Vec<CertificateDer<'static>>>().ok())
            .and_then(|chain| chain.into_iter().next());
        let (mut driver, send_request) = client::new(Connection::new(connection)).await?;
        let driver = tokio::spawn(async move {
            // Resolves with the closing error once the connection ends; that is
            // the expected terminal state, not a failure.
            let _closed = poll_fn(|cx| driver.poll_close(cx)).await;
        });
        Ok(Http3Client {
            server_leaf,
            endpoint,
            send_request,
            driver,
        })
    }

    /// The leaf certificate the server presented on this connection.
    #[must_use]
    pub const fn server_leaf(&self) -> Option<&CertificateDer<'static>> {
        self.server_leaf.as_ref()
    }

    /// Issue one request with an optional body and read the full response.
    ///
    /// # Errors
    ///
    /// Returns [`Http3ClientError::Stream`] when the request stream fails,
    /// the client refusing to send a header section over the server's
    /// advertised limit included.
    pub async fn send(&mut self, request: Request<()>, body: &[u8]) -> Result<Http3Response, Http3ClientError> {
        Http3Client::exchange(self.send_request.clone(), request, body).await
    }

    /// A cloned opener that can drive a request concurrently with others.
    #[must_use]
    pub fn opener(&self) -> SendRequest<OpenStreams, Bytes> {
        self.send_request.clone()
    }

    /// Drive one request/response exchange on `send_request`.
    ///
    /// # Errors
    ///
    /// Returns [`Http3ClientError::Stream`] when the request stream fails.
    pub async fn exchange(mut send_request: SendRequest<OpenStreams, Bytes>, request: Request<()>, body: &[u8]) -> Result<Http3Response, Http3ClientError> {
        let mut stream = send_request.send_request(request).await?;
        if !body.is_empty() {
            stream.send_data(Bytes::copy_from_slice(body)).await?;
        }
        stream.finish().await?;
        let response = stream.recv_response().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let mut collected = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await? {
            while chunk.has_remaining() {
                let slice = chunk.chunk();
                collected.extend_from_slice(slice);
                let consumed = slice.len();
                chunk.advance(consumed);
            }
        }
        Ok(Http3Response {
            status,
            headers,
            body: Bytes::from(collected),
        })
    }

    /// Close the connection and wait for its driver to finish.
    pub async fn close(self) {
        drop(self.send_request);
        let _joined = self.driver.await;
        self.endpoint.wait_idle().await;
    }
}

/// Build an HTTP/3 request for `path` on the loopback authority at `port`.
///
/// # Errors
///
/// Returns [`Http3ClientError::Request`] when `path` does not form a URI.
pub fn request(method: Method, port: u16, path: &str) -> Result<Request<()>, Http3ClientError> {
    Ok(Request::builder().method(method).uri(format!("https://localhost:{port}{path}")).body(())?)
}
