//! One request header section limit across the broker's three HTTP versions.

use crate::listener_tls::SharedTlsIdentity;
use andromeda_configuration::schema::{listener_transport::ListenerTransport, server::ServerConfig, tls::TlsConfig};
use andromeda_http_server::{close_policy::ClosePolicy, execution::serve_bound};
use andromeda_http3_server::{endpoint, execution::serve_endpoint, quic_crypto::H3_ALPN};
use andromeda_server_tls::test_identity::{authority::TestAuthority, identity_files::IdentityFiles};
use axum::{Router, routing::get};
use bytes::{Buf, Bytes};
use h3::{
    client::{self, SendRequest},
    error::StreamError,
};
use h3_quinn::{Connection, OpenStreams};
use http::{Request, StatusCode, Version};
use pretty_assertions::assert_eq;
use quinn::{ClientConfig, Endpoint, crypto::rustls::QuicClientConfig};
use reqwest::{Certificate, Client};
use rustls::crypto::aws_lc_rs::default_provider;
use std::{
    error::Error,
    future::poll_fn,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

/// The boxed error the test returns.
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

/// The answer one client got: a status and body, or a refusal to send.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The listener answered.
    Answered(StatusCode, String),
    /// The client refused to send a section over the listener's advertised
    /// limit (RFC 9114 section 7.2.4.2).
    NotSent,
}

/// Request `/?q=` with a query value of `length` octets over HTTP/1.1 or
/// HTTP/2 and report what came back.
async fn over_tcp(client: &Client, port: u16, version: Version, length: usize) -> TestResult<Outcome> {
    let response = client.get(format!("https://localhost:{port}/?q={}", "a".repeat(length))).send().await?;
    assert_eq!(response.version(), version);
    Ok(Outcome::Answered(response.status(), response.text().await?))
}

/// An HTTP/3 client of the QUIC listener at `server`, trusting only
/// `authority`: the endpoint, the request opener, and the task driving the
/// connection.
async fn quic_client(authority: &TestAuthority, server: SocketAddr) -> TestResult<(Endpoint, SendRequest<OpenStreams, Bytes>, JoinHandle<()>)> {
    let mut crypto = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(authority.root_store()?)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![H3_ALPN.to_vec()];
    let mut endpoint = Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?)));
    let connection = endpoint.connect(server, "localhost")?.await?;
    let (mut driver, opener) = client::new(Connection::new(connection)).await?;
    let driver = tokio::spawn(async move {
        // Resolves with the closing error once the connection ends; that is
        // the expected terminal state, not a failure.
        let _closed = poll_fn(|cx| driver.poll_close(cx)).await;
    });
    Ok((endpoint, opener, driver))
}

/// Send `request` on `opener` and read its status and whole body.
async fn exchange(opener: &mut SendRequest<OpenStreams, Bytes>, request: Request<()>) -> Result<(StatusCode, Vec<u8>), StreamError> {
    let mut stream = opener.send_request(request).await?;
    stream.finish().await?;
    let status = stream.recv_response().await?.status();
    let mut body = Vec::new();
    while let Some(mut chunk) = stream.recv_data().await? {
        let remaining = chunk.remaining();
        body.extend_from_slice(&chunk.copy_to_bytes(remaining));
    }
    Ok((status, body))
}

/// Request `/?q=` with a query value of `length` octets over HTTP/3 and
/// report what came back.
async fn over_quic(opener: &mut SendRequest<OpenStreams, Bytes>, port: u16, length: usize) -> TestResult<Outcome> {
    let request = Request::get(format!("https://localhost:{port}/?q={}", "a".repeat(length))).body(())?;
    match exchange(opener, request).await {
        Ok((status, body)) => Ok(Outcome::Answered(status, String::from_utf8(body)?)),
        Err(StreamError::HeaderTooBig { .. }) => Ok(Outcome::NotSent),
        Err(error) => Err(error.into()),
    }
}

/// The default limit is 32 KiB. A query 1 KiB under it fits whatever fields
/// each client adds, and a 32 KiB query alone is over it.
#[tokio::test]
async fn one_long_query_is_served_or_refused_identically_over_http11_http2_and_http3() -> TestResult {
    let authority = TestAuthority::generate()?;
    let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
    let tls = TlsConfig {
        certificate_path: files.certificate_path().to_path_buf(),
        private_key_path: files.private_key_path().to_path_buf(),
        ..TlsConfig::default()
    };
    let server = ServerConfig {
        transport: ListenerTransport::Tls,
        ..ServerConfig::default()
    };
    let limit = server.max_request_header_section;
    let policy = ClosePolicy::from(&server);
    let mut identity = SharedTlsIdentity::new(&tls);
    let security = identity.tcp_security(ListenerTransport::Tls)?;
    let feed = identity.quic_feed()?;
    let router = Router::new().route("/", get(|| async { "served" }));

    let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let tcp_port = tcp.local_addr()?.port();
    let (tcp_stop, tcp_stopped) = oneshot::channel::<()>();
    // clone: the TCP and the QUIC listener each serve the one router.
    let tcp_router = router.clone();
    let tcp_server = tokio::spawn(async move {
        serve_bound(&server, tcp, security, tcp_router, async move {
            let _ = tcp_stopped.await;
        })
        .await
    });
    let quic = endpoint::bind(&feed, SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let quic_address = quic.local_addr()?;
    let (quic_stop, quic_stopped) = oneshot::channel::<()>();
    let quic_server = tokio::spawn(serve_endpoint(quic, feed, router, limit, policy, async move {
        let _ = quic_stopped.await;
    }));

    let trusting = || {
        Ok::<_, reqwest::Error>(
            Client::builder()
                .tls_certs_only([Certificate::from_der(authority.certificate())?])
                .resolve("localhost", SocketAddr::from((Ipv4Addr::LOCALHOST, 0))),
        )
    };
    let http1 = trusting()?.http1_only().build()?;
    let http2 = trusting()?.build()?;
    let (quic_client_endpoint, mut http3, quic_driver) = quic_client(&authority, quic_address).await?;

    let under = usize::from(limit.octets()) - 1024;
    let ok = Outcome::Answered(StatusCode::OK, "served".to_owned());
    assert_eq!(over_tcp(&http1, tcp_port, Version::HTTP_11, under).await?, ok);
    assert_eq!(over_tcp(&http2, tcp_port, Version::HTTP_2, under).await?, ok);
    assert_eq!(over_quic(&mut http3, quic_address.port(), under).await?, ok);

    let over = usize::from(limit.octets());
    let refused = Outcome::Answered(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE, String::new());
    assert_eq!(over_tcp(&http1, tcp_port, Version::HTTP_11, over).await?, refused);
    assert_eq!(over_tcp(&http2, tcp_port, Version::HTTP_2, over).await?, refused);
    let over_http3 = over_quic(&mut http3, quic_address.port(), over).await?;
    assert!(over_http3 == refused || over_http3 == Outcome::NotSent, "{over_http3:?}");

    drop(http3);
    quic_driver.await?;
    quic_client_endpoint.wait_idle().await;
    tcp_stop.send(()).map_err(|()| "the TCP listener stopped early")?;
    quic_stop.send(()).map_err(|()| "the QUIC listener stopped early")?;
    tcp_server.await??;
    quic_server.await??;
    drop(identity);
    Ok(())
}
