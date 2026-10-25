//! `Strict-Transport-Security` on the API reaches a client only over the TLS listener.

use crate::listener_tls::{SharedTlsIdentity, listener_scheme};
use andromeda_http_server::{
    config::{ListenerTransport, ServerConfig},
    execution::serve_bound,
};
use andromeda_http_stack::{config::HttpStackConfig, stack::HttpStack};
use andromeda_server_tls::{
    config::TlsConfig,
    test_identity::{authority::TestAuthority, identity_files::IdentityFiles},
};
use axum::{Router, routing::get};
use http::{StatusCode, header};
use pretty_assertions::assert_eq;
use reqwest::{Certificate, Client};
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr},
};
use tokio::{net::TcpListener, sync::oneshot};
use tower_http::add_extension::AddExtensionLayer;

/// The boxed error the test returns.
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

/// The `Strict-Transport-Security` value a client receives for `/` on a listener
/// of `transport`, or `None` when the response carries no such header.
///
/// The listener serves the API stack the way the broker assembles it, with the
/// listener's scheme installed as the outermost request extension.
async fn policy_seen_over(stack: &HttpStack, transport: ListenerTransport, tls: &TlsConfig, trusted: Option<Certificate>) -> TestResult<Option<String>> {
    let server = ServerConfig {
        transport,
        ..ServerConfig::default()
    };
    let mut identity = SharedTlsIdentity::new(tls);
    let security = identity.tcp_security(transport)?;
    let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let port = tcp.local_addr()?.port();
    let router = stack
        .apply_api(Router::new().route("/", get(|| async { StatusCode::OK })))
        .layer(AddExtensionLayer::new(listener_scheme(transport)));

    let (stop, stopped) = oneshot::channel::<()>();
    let listener = tokio::spawn(async move {
        serve_bound(&server, tcp, security, router, async move {
            let _ = stopped.await;
        })
        .await
    });

    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let builder = Client::builder().resolve("localhost", address);
    let client = match trusted {
        Some(certificate) => builder.tls_certs_only([certificate]).build()?,
        None => builder.build()?,
    };
    let scheme = match transport {
        ListenerTransport::Plaintext => "http",
        ListenerTransport::Tls => "https",
    };
    let response = client.get(format!("{scheme}://localhost:{port}/")).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    let policy = response
        .headers()
        .get(header::STRICT_TRANSPORT_SECURITY)
        .map(|value| value.to_str().map(str::to_owned))
        .transpose()?;

    stop.send(()).map_err(|()| "the listener stopped early")?;
    listener.await??;
    drop(identity);
    Ok(policy)
}

#[tokio::test]
async fn strict_transport_security_reaches_a_client_over_tls_and_not_over_plaintext() -> TestResult {
    let authority = TestAuthority::generate()?;
    let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
    let tls = TlsConfig {
        certificate_path: files.certificate_path().to_path_buf(),
        private_key_path: files.private_key_path().to_path_buf(),
        ..TlsConfig::default()
    };
    let stack = HttpStack::new(HttpStackConfig::default())?;
    let trusted = Certificate::from_der(authority.certificate())?;

    let over_tls = policy_seen_over(&stack, ListenerTransport::Tls, &tls, Some(trusted)).await?;
    let over_plaintext = policy_seen_over(&stack, ListenerTransport::Plaintext, &tls, None).await?;

    assert_eq!(over_tls.as_deref(), Some("max-age=15552000; includeSubDomains"));
    assert_eq!(over_plaintext, None);
    Ok(())
}
