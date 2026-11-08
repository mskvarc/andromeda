//! Real handshakes against a TLS-terminating TCP listener.

use crate::{
    close_policy::ClosePolicy,
    connection_bounds::ConnectionBounds,
    error::HttpServerError,
    execution::serve_listener,
    field_section::HeaderSectionPolicy,
    listener_security::TCP_HANDSHAKE_POLICY,
    tls_tcp_listener::TlsTcpListener,
};
use andromeda_configuration::{
    header_section_limit::HeaderSectionLimit,
    human_duration::HumanDuration,
    schema::{
        listener_transport::ListenerTransport,
        tls::{ClientAuthenticationConfig, ClientAuthenticationMode, MinimumProtocolVersion, TlsConfig},
    },
};
use andromeda_server_tls::{
    material_watch::TlsMaterialWatch,
    server_config_feed::ServerConfigFeed,
    test_identity::{authority::TestAuthority, identity::TestIdentity, identity_files::IdentityFiles},
};
use axum::{Router, routing::get};
use reqwest::{Certificate, Client, ClientBuilder, Identity, Response, StatusCode, Version, tls};
use std::{error::Error, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::JoinHandle,
    time::timeout,
};

/// The boxed error every test returns.
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

/// The handshake deadline the listener runs under in these tests.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(200);

/// How the listener under test treats client certificates.
enum ClientPolicy<'authority> {
    /// No client certificate is requested.
    Anonymous,
    /// Certificates issued by the authority are verified when presented.
    Optional(&'authority TestAuthority),
    /// A certificate issued by the authority is required.
    Required(&'authority TestAuthority),
}

/// A TLS listener serving a router on an ephemeral loopback port.
struct RunningTlsListener {
    /// Where the listener accepts connections.
    address: SocketAddr,
    /// The authority that issued the server's certificate.
    authority: TestAuthority,
    /// The server identity files, replaceable to rotate the certificate.
    files: IdentityFiles,
    /// A clone of the listener's feed, to observe rebuilds.
    feed: ServerConfigFeed,
    /// Keeps the material watched for the listener's lifetime.
    _watch: TlsMaterialWatch,
    /// Signals graceful shutdown.
    shutdown: Option<oneshot::Sender<()>>,
    /// The serving task.
    server: JoinHandle<Result<(), HttpServerError>>,
}

impl RunningTlsListener {
    /// Serve `router` over TLS under the given version floor and client policy.
    async fn start(router: Router, minimum_protocol_version: MinimumProtocolVersion, clients: ClientPolicy<'_>) -> TestResult<RunningTlsListener> {
        let authority = TestAuthority::generate()?;
        let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
        let (client_authentication, client_ca_path) = match clients {
            ClientPolicy::Anonymous => (ClientAuthenticationMode::Disabled, None),
            ClientPolicy::Optional(issuer) => (ClientAuthenticationMode::Optional, Some(files.write_client_ca(issuer)?)),
            ClientPolicy::Required(issuer) => (ClientAuthenticationMode::Required, Some(files.write_client_ca(issuer)?)),
        };
        let config = TlsConfig {
            certificate: Some(files.certificate_path().to_path_buf()),
            private_key: Some(files.private_key_path().to_path_buf()),
            client_authentication: ClientAuthenticationConfig {
                mode: client_authentication,
                ca: client_ca_path,
            },
            minimum_protocol_version,
            handshake_timeout: HumanDuration::new(HANDSHAKE_TIMEOUT),
            reload_settle_after: HumanDuration::from_millis(100),
        };
        let watch = TlsMaterialWatch::start(&config)?;
        let feed = watch.feed(TCP_HANDSHAKE_POLICY)?;
        let socket = TcpListener::bind("127.0.0.1:0").await?;
        let address = socket.local_addr()?;
        let listener = TlsTcpListener::new(socket, feed.clone(), HANDSHAKE_TIMEOUT);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_listener(
            listener,
            router,
            async move {
                match shutdown_receiver.await {
                    Ok(()) | Err(_) => {}
                }
            },
            ClosePolicy {
                drain_deadline: Duration::from_secs(2),
                announced_close_delay: Duration::from_millis(500),
            },
            ConnectionBounds {
                head: Duration::from_secs(30),
                idle: Duration::from_secs(180),
            },
            HeaderSectionPolicy {
                limit: HeaderSectionLimit::MAXIMUM,
                transport: ListenerTransport::Tls,
            },
        ));
        Ok(RunningTlsListener {
            address,
            authority,
            files,
            feed,
            _watch: watch,
            shutdown: Some(shutdown),
            server,
        })
    }

    /// Serve the plain test router with no client certificates requested.
    async fn anonymous() -> TestResult<RunningTlsListener> {
        RunningTlsListener::start(test_router(), MinimumProtocolVersion::Tls12, ClientPolicy::Anonymous).await
    }

    /// The HTTPS URL of `path` on this listener.
    fn url(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.address.port())
    }

    /// A client builder trusting only this listener's authority.
    fn client(&self) -> TestResult<ClientBuilder> {
        trusting(&self.authority)
    }

    /// Signal shutdown and wait for the server to drain and stop.
    async fn stop(mut self) -> TestResult {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.send(()).map_err(|()| "the server stopped early")?;
        }
        self.server.await??;
        Ok(())
    }
}

/// A router answering `/test`.
fn test_router() -> Router {
    Router::new().route("/test", get(|| async { "served" }))
}

/// A client builder trusting only `authority`, resolving `localhost` to loopback
/// and reporting the peer certificate of each response.
fn trusting(authority: &TestAuthority) -> TestResult<ClientBuilder> {
    Ok(Client::builder()
        .tls_certs_only([Certificate::from_der(authority.certificate())?])
        .tls_info(true)
        .resolve("localhost", SocketAddr::from(([127, 0, 0, 1], 0))))
}

/// A client presenting `identity` as its certificate.
fn presenting(builder: ClientBuilder, identity: &TestIdentity) -> TestResult<Client> {
    Ok(builder.identity(Identity::from_pem(identity.identity_pem().as_bytes())?).build()?)
}

/// The leaf certificate the server presented on the connection `response` came over.
fn presented_leaf(response: &Response) -> Option<Vec<u8>> {
    response
        .extensions()
        .get::<tls::TlsInfo>()
        .and_then(tls::TlsInfo::peer_certificate)
        .map(<[u8]>::to_vec)
}

/// Request `/test` and return the response body.
async fn fetch(client: &Client, running: &RunningTlsListener) -> TestResult<String> {
    Ok(client.get(running.url("/test")).send().await?.error_for_status()?.text().await?)
}

#[tokio::test]
async fn a_tls_listener_serves_a_trusting_client() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    pretty_assertions::assert_eq!(fetch(&running.client()?.build()?, &running).await?, "served");
    running.stop().await
}

#[tokio::test]
async fn alpn_negotiates_http2() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    let response = running.client()?.build()?.get(running.url("/test")).send().await?;
    pretty_assertions::assert_eq!(response.version(), Version::HTTP_2);
    pretty_assertions::assert_eq!(response.text().await?, "served");
    running.stop().await
}

#[tokio::test]
async fn an_http11_only_client_is_served() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    let response = running.client()?.http1_only().build()?.get(running.url("/test")).send().await?;
    pretty_assertions::assert_eq!(response.version(), Version::HTTP_11);
    pretty_assertions::assert_eq!(response.text().await?, "served");
    running.stop().await
}

#[tokio::test]
async fn a_plaintext_request_fails_and_the_listener_keeps_serving() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    let plaintext = Client::builder().build()?.get(format!("http://{}/test", running.address)).send().await;
    assert!(plaintext.is_err(), "a plaintext request gets no HTTP response: {plaintext:?}");
    pretty_assertions::assert_eq!(fetch(&running.client()?.build()?, &running).await?, "served");
    running.stop().await
}

#[tokio::test]
async fn a_client_not_trusting_the_certificate_is_rejected() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    let stranger = trusting(&TestAuthority::generate()?)?.build()?;
    assert!(
        stranger.get(running.url("/test")).send().await.is_err(),
        "an untrusted certificate fails the handshake"
    );
    running.stop().await
}

/// Clause 6.2 requires TLS 1.2 support, which the default floor provides.
#[tokio::test]
async fn a_tls12_only_client_is_served_under_the_tls12_floor() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    let client = running.client()?.tls_version_max(tls::Version::TLS_1_2).build()?;
    pretty_assertions::assert_eq!(fetch(&client, &running).await?, "served");
    running.stop().await
}

#[tokio::test]
async fn a_tls12_only_client_is_rejected_under_the_tls13_floor() -> TestResult {
    let running = RunningTlsListener::start(test_router(), MinimumProtocolVersion::Tls13, ClientPolicy::Anonymous).await?;
    let tls12 = running.client()?.tls_version_max(tls::Version::TLS_1_2).build()?;
    assert!(tls12.get(running.url("/test")).send().await.is_err(), "TLS 1.2 is below the floor");
    let tls13 = running.client()?.build()?;
    pretty_assertions::assert_eq!(fetch(&tls13, &running).await?, "served");
    running.stop().await
}

#[tokio::test]
async fn a_stalled_handshake_times_out_without_blocking_others() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    let mut stalled = TcpStream::connect(running.address).await?;
    pretty_assertions::assert_eq!(fetch(&running.client()?.build()?, &running).await?, "served");
    let mut buffer = [0_u8; 1];
    let outcome = timeout(HANDSHAKE_TIMEOUT * 10, stalled.read(&mut buffer)).await?;
    assert!(matches!(outcome, Ok(0) | Err(_)), "the stalled connection is closed: {outcome:?}");
    running.stop().await
}

#[tokio::test]
async fn required_client_authentication_rejects_a_client_without_a_certificate_and_serves_one_with_a_ca_signed_certificate() -> TestResult {
    let clients = TestAuthority::generate()?;
    let running = RunningTlsListener::start(test_router(), MinimumProtocolVersion::Tls12, ClientPolicy::Required(&clients)).await?;
    let anonymous = running.client()?.build()?;
    assert!(
        anonymous.get(running.url("/test")).send().await.is_err(),
        "a client without a certificate is refused"
    );
    let authenticated = presenting(running.client()?, &clients.issue_client_identity()?)?;
    pretty_assertions::assert_eq!(fetch(&authenticated, &running).await?, "served");
    running.stop().await
}

#[tokio::test]
async fn optional_client_authentication_serves_anonymous_and_ca_signed_clients_and_rejects_an_unknown_ca() -> TestResult {
    let clients = TestAuthority::generate()?;
    let running = RunningTlsListener::start(test_router(), MinimumProtocolVersion::Tls12, ClientPolicy::Optional(&clients)).await?;
    pretty_assertions::assert_eq!(fetch(&running.client()?.build()?, &running).await?, "served");
    let authenticated = presenting(running.client()?, &clients.issue_client_identity()?)?;
    pretty_assertions::assert_eq!(fetch(&authenticated, &running).await?, "served");
    let impostor = presenting(running.client()?, &TestAuthority::generate()?.issue_client_identity()?)?;
    assert!(
        impostor.get(running.url("/test")).send().await.is_err(),
        "a certificate from an unknown CA is refused"
    );
    running.stop().await
}

#[tokio::test]
async fn a_rotated_certificate_is_presented_to_new_connections_while_an_open_connection_survives() -> TestResult {
    let running = RunningTlsListener::anonymous().await?;
    let original = running.feed.current();
    let open = running.client()?.build()?;
    let first = open.get(running.url("/test")).send().await?;
    let first_leaf = presented_leaf(&first);
    pretty_assertions::assert_eq!(first.text().await?, "served");

    let rotated = running.authority.issue_server_identity()?;
    running.files.replace(&rotated)?;
    let mut feed = running.feed.clone();
    while Arc::ptr_eq(&original, &feed.current()) {
        timeout(Duration::from_secs(5), feed.changed()).await?.ok_or("the feed closed")?;
    }

    let fresh = running.client()?.build()?.get(running.url("/test")).send().await?;
    pretty_assertions::assert_eq!(
        presented_leaf(&fresh),
        Some(rotated.certificate().to_vec()),
        "a new connection sees the rotated leaf"
    );
    let reused = open.get(running.url("/test")).send().await?;
    pretty_assertions::assert_eq!(reused.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(presented_leaf(&reused), first_leaf, "the open connection keeps its session");
    running.stop().await
}

#[tokio::test]
async fn graceful_shutdown_drains_an_in_flight_tls_request() -> TestResult {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let handler_started = Arc::clone(&started);
    let handler_release = Arc::clone(&release);
    let router = Router::new().route(
        "/slow",
        get(move || {
            let started = Arc::clone(&handler_started);
            let release = Arc::clone(&handler_release);
            async move {
                started.notify_one();
                release.notified().await;
                "complete"
            }
        }),
    );
    let mut running = RunningTlsListener::start(router, MinimumProtocolVersion::Tls12, ClientPolicy::Anonymous).await?;
    let request = tokio::spawn(running.client()?.build()?.get(running.url("/slow")).send());
    started.notified().await;
    running
        .shutdown
        .take()
        .ok_or("already stopped")?
        .send(())
        .map_err(|()| "the server stopped early")?;
    release.notify_one();
    pretty_assertions::assert_eq!(request.await??.text().await?, "complete");
    running.stop().await
}
