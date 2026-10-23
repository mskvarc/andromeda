//! One request header section limit across the broker's three HTTP versions.

use crate::listener_tls::SharedTlsIdentity;
use andromeda_http_server::{
    config::{ListenerTransport, ServerConfig},
    execution::serve_bound,
};
use andromeda_http3_server::{endpoint, execution::serve_endpoint, quic_crypto::H3_ALPN};
use andromeda_server_tls::config::TlsConfig;
use andromeda_test_http3::{
    client::{Http3Client, request},
    error::Http3ClientError,
};
use andromeda_test_tls::{authority::TestAuthority, identity_files::IdentityFiles};
use axum::{Router, routing::get};
use h3::error::StreamError;
use http::{Method, StatusCode, Version};
use pretty_assertions::assert_eq;
use reqwest::{Certificate, Client};
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr},
};
use tokio::{net::TcpListener, sync::oneshot};

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

/// Request `/?q=` with a query value of `length` octets over HTTP/3 and
/// report what came back.
async fn over_quic(client: &mut Http3Client, port: u16, length: usize) -> TestResult<Outcome> {
    match client.send(request(Method::GET, port, &format!("/?q={}", "a".repeat(length)))?, &[]).await {
        Ok(response) => Ok(Outcome::Answered(response.status, response.text()?)),
        Err(Http3ClientError::Stream(StreamError::HeaderTooBig { .. })) => Ok(Outcome::NotSent),
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
    let quic_server = tokio::spawn(serve_endpoint(quic, feed, router, limit, async move {
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
    let mut http3 = Http3Client::connect(authority.root_store()?, vec![H3_ALPN.to_vec()], quic_address, None).await?;

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

    http3.close().await;
    tcp_stop.send(()).map_err(|()| "the TCP listener stopped early")?;
    quic_stop.send(()).map_err(|()| "the QUIC listener stopped early")?;
    tcp_server.await??;
    quic_server.await??;
    drop(identity);
    Ok(())
}
