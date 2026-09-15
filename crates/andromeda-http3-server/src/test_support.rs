//! Self-signed TLS material and an HTTP/3 client harness shared across tests.
//!
//! HTTP/3 cannot be exercised without a certificate, so every test that binds
//! or dials a QUIC endpoint needs a throwaway key pair; generating it once here
//! keeps that setup out of the individual test modules. The [`Http3Client`]
//! harness dials such a listener and issues real requests over QUIC, so the
//! server modules can be tested over the wire rather than through mocks.

use crate::config::{Http3Config, Http3Mode};
use bytes::{Buf, Bytes};
use h3::client::{self, SendRequest};
use h3_quinn::{Connection, OpenStreams};
use http::{HeaderMap, Request, StatusCode};
use quinn::{ClientConfig, Endpoint, crypto::rustls::QuicClientConfig};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::{RootCertStore, crypto::aws_lc_rs::default_provider};
use rustls_pki_types::CertificateDer;
use std::{
    error::Error,
    fs::File,
    future::poll_fn,
    io::Write,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tempfile::TempDir;
use tokio::task::JoinHandle;

/// The ALPN token for HTTP/3, as a client offers it.
pub(crate) const H3_ALPN: &[u8] = b"h3";

/// A generated self-signed identity written to disk, with the paths wired into
/// an enabled [`Http3Config`] and the leaf certificate kept for client trust.
pub(crate) struct SelfSignedListener {
    /// Keeps the certificate and key files alive for the listener's lifetime.
    _directory: TempDir,
    /// An enabled configuration pointing at the written key pair.
    config: Http3Config,
    /// The leaf certificate, for a dialling client's root store.
    certificate: CertificateDer<'static>,
}

impl SelfSignedListener {
    /// Generate a `localhost` identity and write it to a fresh temporary directory.
    pub(crate) fn generate() -> Result<SelfSignedListener, Box<dyn Error + Send + Sync>> {
        let directory = TempDir::new()?;
        let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec!["localhost".to_owned()])?;
        let certificate_path = directory.path().join("broker.crt");
        let private_key_path = directory.path().join("broker.key");
        File::create(&certificate_path)?.write_all(cert.pem().as_bytes())?;
        File::create(&private_key_path)?.write_all(key_pair.serialize_pem().as_bytes())?;
        let config = Http3Config {
            mode: Http3Mode::Enabled,
            certificate_path,
            private_key_path,
            ..Http3Config::default()
        };
        Ok(SelfSignedListener {
            _directory: directory,
            config,
            certificate: cert.der().clone(),
        })
    }

    /// The enabled configuration referencing the written key pair.
    pub(crate) fn config(&self) -> &Http3Config {
        &self.config
    }

    /// The leaf certificate a dialling client must trust as its sole root.
    pub(crate) fn certificate(&self) -> CertificateDer<'static> {
        self.certificate.clone()
    }

    /// A root store trusting only this listener's certificate.
    pub(crate) fn root_store(&self) -> Result<RootCertStore, Box<dyn Error + Send + Sync>> {
        let mut roots = RootCertStore::empty();
        roots.add(self.certificate())?;
        Ok(roots)
    }

    /// Dial this listener over HTTP/3, trusting only its certificate.
    pub(crate) async fn dial(&self, server: SocketAddr) -> Result<Http3Client, Box<dyn Error + Send + Sync>> {
        Http3Client::connect(self.root_store()?, vec![H3_ALPN.to_vec()], server).await
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
    /// The client-side QUIC endpoint, kept alive for the connection's lifetime.
    endpoint: Endpoint,
    /// The request opener, cloneable to multiplex concurrent request streams.
    send_request: SendRequest<OpenStreams, Bytes>,
    /// The connection driver task, awaited on close.
    driver: JoinHandle<()>,
}

impl Http3Client {
    /// Connect to `server`, verifying its certificate against `roots` and
    /// offering the given ALPN protocols.
    pub(crate) async fn connect(roots: RootCertStore, alpn: Vec<Vec<u8>>, server: SocketAddr) -> Result<Http3Client, Box<dyn Error + Send + Sync>> {
        let mut crypto = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        crypto.alpn_protocols = alpn;
        let client_config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));
        let mut endpoint = Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        endpoint.set_default_client_config(client_config);
        let connection = endpoint.connect(server, "localhost")?.await?;
        let (mut driver, send_request) = client::new(Connection::new(connection)).await?;
        let driver = tokio::spawn(async move {
            // Resolves with the closing error once the connection ends; that is
            // the expected terminal state, not a failure.
            let _closed = poll_fn(|cx| driver.poll_close(cx)).await;
        });
        Ok(Http3Client {
            endpoint,
            send_request,
            driver,
        })
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
