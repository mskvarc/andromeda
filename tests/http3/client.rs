//! An HTTP/3 client for driving the live broker over QUIC.
//!
//! Unlike the other integration suites, which reach the broker over TCP with
//! `reqwest`, this suite must speak HTTP/3, so it dials the broker's QUIC
//! endpoint directly with `quinn` + `h3`. The broker terminates TLS itself, so
//! the client trusts the same self-signed certificate the compose stack mounts
//! into the broker (its path, the QUIC authority, and the TLS server name are
//! all overridable by environment variable for other deployments).

use bytes::{Buf, Bytes};
use h3::client::{self, SendRequest};
use h3_quinn::{Connection, OpenStreams};
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
use quinn::{ClientConfig, Endpoint, crypto::rustls::QuicClientConfig};
use rustls::{RootCertStore, crypto::aws_lc_rs::default_provider};
use rustls_pki_types::{CertificateDer, pem::PemObject};
use std::{
    env,
    error::Error,
    fs,
    future::poll_fn,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{task::JoinHandle, time::sleep};

/// The QUIC authority (`host:port`) the broker's HTTP/3 listener is published on.
const AUTHORITY_ENV: &str = "ANDROMEDA_TEST_HTTP3_AUTHORITY";
/// The PEM certificate the broker presents, to trust as the sole root.
const CERTIFICATE_ENV: &str = "ANDROMEDA_TEST_HTTP3_CERT";
/// The TLS server name to verify the certificate against.
const SERVER_NAME_ENV: &str = "ANDROMEDA_TEST_HTTP3_SERVER_NAME";
/// The base URL of the broker's TCP listener, for cross-transport assertions.
const TCP_BASE_ENV: &str = "ANDROMEDA_TEST_BROKER_URL";

/// The ALPN token HTTP/3 negotiates (RFC 9114 clause 3.1).
pub const H3_ALPN: &[u8] = b"h3";

/// The QUIC authority the broker is reached at.
pub fn broker_authority() -> SocketAddr {
    env::var(AUTHORITY_ENV)
        .unwrap_or_else(|_| "127.0.0.1:1056".to_owned())
        .parse()
        .expect("ANDROMEDA_TEST_HTTP3_AUTHORITY must be host:port")
}

/// The TLS server name presented to the broker (matched against its SANs).
pub fn server_name() -> String {
    env::var(SERVER_NAME_ENV).unwrap_or_else(|_| "localhost".to_owned())
}

/// The broker's TCP base URL, trailing slash trimmed.
pub fn tcp_base() -> String {
    env::var(TCP_BASE_ENV)
        .unwrap_or_else(|_| "http://127.0.0.1:1056/ngsi-ld/v1/".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

/// A root store trusting only the broker's mounted certificate.
pub fn trusted_roots() -> RootCertStore {
    let path = env::var(CERTIFICATE_ENV).unwrap_or_else(|_| "docker/http3/certs/broker.crt".to_owned());
    let pem = fs::read(&path).unwrap_or_else(|error| panic!("read broker certificate {path} (generate it per docker/http3 header): {error}"));
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(&pem) {
        roots
            .add(certificate.expect("certificate PEM parses"))
            .expect("certificate is added to the root store");
    }
    roots
}

/// A response read in full from an HTTP/3 request stream.
pub struct Http3Response {
    /// The response status.
    pub status: StatusCode,
    /// The response headers.
    pub headers: HeaderMap,
    /// The response body, concatenated across QUIC data frames.
    pub body: Bytes,
}

impl Http3Response {
    /// The body decoded as UTF-8 text.
    pub fn text(&self) -> String {
        String::from_utf8(self.body.to_vec()).expect("response body is UTF-8")
    }

    /// The body parsed as JSON.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("response body is JSON")
    }
}

/// An open HTTP/3 connection to the broker, reusable for several requests.
pub struct Http3Client {
    /// The client-side QUIC endpoint, kept alive for the connection's lifetime.
    endpoint: Endpoint,
    /// The request opener, cloned per request to multiplex streams.
    send_request: SendRequest<OpenStreams, Bytes>,
    /// The connection driver task, awaited on close.
    driver: JoinHandle<()>,
}

impl Http3Client {
    /// Connect trusting the broker's certificate and offering the `h3` ALPN.
    pub async fn connect_trusted() -> Http3Client {
        Http3Client::connect(trusted_roots(), vec![H3_ALPN.to_vec()])
            .await
            .expect("HTTP/3 connection to the broker is established")
    }

    /// Connect verifying the broker against `roots` and offering `alpn`.
    pub async fn connect(roots: RootCertStore, alpn: Vec<Vec<u8>>) -> Result<Http3Client, Box<dyn Error + Send + Sync>> {
        let mut crypto = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        crypto.alpn_protocols = alpn;
        let client_config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));
        let mut endpoint = Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))?;
        endpoint.set_default_client_config(client_config);
        let connection = endpoint.connect(broker_authority(), &server_name())?.await?;
        let (mut driver, send_request) = client::new(Connection::new(connection)).await?;
        let driver = tokio::spawn(async move {
            let _closed = poll_fn(|cx| driver.poll_close(cx)).await;
        });
        Ok(Http3Client {
            endpoint,
            send_request,
            driver,
        })
    }

    /// Issue one request with the given headers and body, reading the full response.
    pub async fn request(&self, method: Method, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Http3Response {
        let uri = format!("https://{}:{}{path}", server_name(), broker_authority().port());
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            let name = HeaderName::from_bytes(name.as_bytes()).expect("header name is valid");
            let value = HeaderValue::from_str(value).expect("header value is valid");
            builder = builder.header(name, value);
        }
        // Declare the length of a fixed-size body, as a real client does; the
        // broker requires it to admit the request body.
        if !body.is_empty() {
            builder = builder.header("content-length", body.len().to_string());
        }
        let request = builder.body(()).expect("request builds");

        let mut send_request = self.send_request.clone();
        let mut stream = send_request.send_request(request).await.expect("request stream opens");
        if !body.is_empty() {
            stream.send_data(Bytes::copy_from_slice(body)).await.expect("request body is sent");
        }
        stream.finish().await.expect("request is finished");
        let response = stream.recv_response().await.expect("response headers arrive");
        let status = response.status();
        let response_headers = response.headers().clone();
        let mut collected = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await.expect("response body frames arrive") {
            while chunk.has_remaining() {
                let slice = chunk.chunk();
                collected.extend_from_slice(slice);
                let consumed = slice.len();
                chunk.advance(consumed);
            }
        }
        Http3Response {
            status,
            headers: response_headers,
            body: Bytes::from(collected),
        }
    }

    /// Close the connection and wait for its driver to finish.
    pub async fn close(self) {
        drop(self.send_request);
        let _joined = self.driver.await;
        self.endpoint.wait_idle().await;
    }
}

/// Wait until the broker answers `GET /ngsi-ld/v1/health` over HTTP/3, or panic.
///
/// The compose stack gates readiness on the TCP health check, but the host may
/// still race the first QUIC handshake, so the suite waits on the HTTP/3 path it
/// actually exercises.
pub async fn wait_until_ready() {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(client) = Http3Client::connect(trusted_roots(), vec![H3_ALPN.to_vec()]).await {
            let response = client.request(Method::GET, "/ngsi-ld/v1/health", &[], &[]).await;
            client.close().await;
            if response.status == StatusCode::OK {
                return;
            }
        }
        assert!(Instant::now() < deadline, "broker HTTP/3 listener was not ready within 30s");
        sleep(Duration::from_millis(200)).await;
    }
}
