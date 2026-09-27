//! TLS material and an HTTP/3 client harness shared across tests.
//!
//! HTTP/3 cannot be exercised without a certificate, so every test that binds
//! or dials a QUIC endpoint needs a throwaway identity served through the same
//! shared-identity feed the broker uses; setting it up once here keeps that out
//! of the individual test modules. The [`Http3Client`] harness dials such a
//! listener and issues real requests over QUIC, so the server modules can be
//! tested over the wire rather than through mocks.

use crate::quic_crypto::{H3_ALPN, QUIC_HANDSHAKE_POLICY};
use andromeda_configuration_scalars::human_duration::HumanDuration;
use andromeda_server_tls::{
    config::{ClientAuthentication, TlsConfig},
    material_watch::TlsMaterialWatch,
    server_config_feed::ServerConfigFeed,
};
use andromeda_test_tls::{authority::TestAuthority, identity::TestIdentity, identity_files::IdentityFiles};
use bytes::{Buf, Bytes};
use h3::client::{self, SendRequest};
use h3_quinn::{Connection, OpenStreams};
use http::{HeaderMap, Request, StatusCode};
use quinn::{ClientConfig, Endpoint, crypto::rustls::QuicClientConfig};
use rustls::{RootCertStore, crypto::aws_lc_rs::default_provider};
use rustls_pki_types::CertificateDer;
use std::{
    error::Error,
    future::poll_fn,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};
use tokio::task::JoinHandle;

/// A server identity on disk, watched and fed to the QUIC endpoint exactly as
/// the broker feeds its shared identity, with the authority that issued it
/// kept for client trust.
pub(crate) struct QuicIdentity {
    /// The live TLS configuration the endpoint serves.
    feed: ServerConfigFeed,
    /// Keeps the identity files watched for the listener's lifetime.
    _watch: TlsMaterialWatch,
    /// The identity files, replaceable to rotate the certificate.
    files: IdentityFiles,
    /// The authority that issued the server's certificate.
    authority: TestAuthority,
}

impl QuicIdentity {
    /// An identity requesting no client certificate.
    ///
    /// Must be called from within a Tokio runtime, which rebuilds the feed.
    pub(crate) fn anonymous() -> Result<QuicIdentity, Box<dyn Error + Send + Sync>> {
        QuicIdentity::start(ClientAuthentication::None, None)
    }

    /// An identity requiring a client certificate issued by `clients`.
    pub(crate) fn requiring_clients_of(clients: &TestAuthority) -> Result<QuicIdentity, Box<dyn Error + Send + Sync>> {
        QuicIdentity::start(ClientAuthentication::Required, Some(clients))
    }

    /// Write a fresh `localhost` identity and feed it under the QUIC policy.
    fn start(client_authentication: ClientAuthentication, clients: Option<&TestAuthority>) -> Result<QuicIdentity, Box<dyn Error + Send + Sync>> {
        let authority = TestAuthority::generate()?;
        let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
        let client_ca_path = clients.map(|issuer| files.write_client_ca(issuer)).transpose()?.unwrap_or_else(PathBuf::new);
        let config = TlsConfig {
            certificate_path: files.certificate_path().to_path_buf(),
            private_key_path: files.private_key_path().to_path_buf(),
            client_authentication,
            client_ca_path,
            reload_debounce: HumanDuration::from_millis(100),
            ..TlsConfig::default()
        };
        let watch = TlsMaterialWatch::start(&config)?;
        let feed = watch.feed(QUIC_HANDSHAKE_POLICY)?;
        Ok(QuicIdentity {
            feed,
            _watch: watch,
            files,
            authority,
        })
    }

    /// The live configuration the endpoint serves.
    pub(crate) const fn feed(&self) -> &ServerConfigFeed {
        &self.feed
    }

    /// A root store trusting only this identity's authority.
    pub(crate) fn root_store(&self) -> Result<RootCertStore, Box<dyn Error + Send + Sync>> {
        Ok(self.authority.root_store()?)
    }

    /// Replace the identity files with a newly issued leaf and return it.
    pub(crate) fn rotate(&self) -> Result<TestIdentity, Box<dyn Error + Send + Sync>> {
        let rotated = self.authority.issue_server_identity()?;
        self.files.replace(&rotated)?;
        Ok(rotated)
    }

    /// Dial the listener over HTTP/3, trusting only this identity's authority.
    pub(crate) async fn dial(&self, server: SocketAddr) -> Result<Http3Client, Box<dyn Error + Send + Sync>> {
        Http3Client::connect(self.root_store()?, vec![H3_ALPN.to_vec()], server, None).await
    }
}

/// A response collected in full from an HTTP/3 request stream.
pub(crate) struct Http3Response {
    /// The response status.
    pub(crate) status: StatusCode,
    /// The response headers.
    pub(crate) headers: HeaderMap,
    /// The response body, concatenated across QUIC data frames.
    pub(crate) body: Bytes,
}

impl Http3Response {
    /// The response body decoded as UTF-8 text.
    pub(crate) fn text(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        Ok(String::from_utf8(self.body.to_vec())?)
    }
}

/// An open HTTP/3 client connection, reusable for several requests.
pub(crate) struct Http3Client {
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
    /// Connect to `server`, verifying its certificate against `roots`,
    /// offering the given ALPN protocols and presenting `identity`, if any.
    pub(crate) async fn connect(
        roots: RootCertStore,
        alpn: Vec<Vec<u8>>,
        server: SocketAddr,
        identity: Option<&TestIdentity>,
    ) -> Result<Http3Client, Box<dyn Error + Send + Sync>> {
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
    pub(crate) const fn server_leaf(&self) -> Option<&CertificateDer<'static>> {
        self.server_leaf.as_ref()
    }

    /// Issue one request with an optional body and read the full response.
    pub(crate) async fn send(&mut self, request: Request<()>, body: &[u8]) -> Result<Http3Response, Box<dyn Error + Send + Sync>> {
        Self::exchange(self.send_request.clone(), request, body).await
    }

    /// A cloned opener that can drive a request concurrently with others.
    pub(crate) fn opener(&self) -> SendRequest<OpenStreams, Bytes> {
        self.send_request.clone()
    }

    /// Drive one request/response exchange on `send_request`.
    pub(crate) async fn exchange(
        mut send_request: SendRequest<OpenStreams, Bytes>,
        request: Request<()>,
        body: &[u8],
    ) -> Result<Http3Response, Box<dyn Error + Send + Sync>> {
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
    pub(crate) async fn close(self) {
        drop(self.send_request);
        let _joined = self.driver.await;
        self.endpoint.wait_idle().await;
    }
}

/// Build an HTTP/3 request for `path` on the loopback authority at `port`.
pub(crate) fn request(method: http::Method, port: u16, path: &str) -> Result<Request<()>, Box<dyn Error + Send + Sync>> {
    Ok(Request::builder().method(method).uri(format!("https://localhost:{port}{path}")).body(())?)
}
