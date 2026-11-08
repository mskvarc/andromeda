//! Connections that carry no request for longer than their bounds allow, over
//! real sockets.

use crate::{
    close_policy::ClosePolicy,
    connection_bounds::ConnectionBounds,
    error::HttpServerError,
    execution::serve_listener,
    field_section::HeaderSectionPolicy,
};
use andromeda_configuration::{header_section_limit::HeaderSectionLimit, schema::listener_transport::ListenerTransport};
use axum::{Router, body::Body, http::StatusCode, routing::get};
use hyper::client::conn::http2::handshake;
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::{error::Error, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::{Instant, sleep},
};

/// What a fixture test returns.
type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

/// The bounds every fixture server runs under.
const BOUNDS: ConnectionBounds = ConnectionBounds {
    head: Duration::from_millis(400),
    idle: Duration::from_millis(1200),
};

/// The close policy every fixture server runs under.
const POLICY: ClosePolicy = ClosePolicy {
    drain_deadline: Duration::from_secs(5),
    announced_close_delay: Duration::from_millis(100),
};

/// A head missing the empty line that ends it.
const HALF_HEAD: &[u8] = b"GET /test HTTP/1.1\r\nHost: localhost\r\n";

/// A server under test, stopped when `shutdown` fires.
struct Fixture {
    /// The address the server listens on.
    address: SocketAddr,
    /// Fires the server's shutdown.
    shutdown: oneshot::Sender<()>,
    /// The serving task.
    server: JoinHandle<Result<(), HttpServerError>>,
}

impl Fixture {
    /// Serve a router answering `/test` with `served` at once and `/slow`
    /// with `slow` after twice the idle bound.
    async fn start() -> Result<Fixture, Box<dyn Error + Send + Sync>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = Router::new().route("/test", get(|| async { "served" })).route(
            "/slow",
            get(|| async {
                sleep(BOUNDS.idle * 2).await;
                "slow"
            }),
        );
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_listener(
            listener,
            router,
            async move {
                match shutdown_receiver.await {
                    Ok(()) | Err(_) => {}
                }
            },
            POLICY,
            BOUNDS,
            HeaderSectionPolicy {
                limit: HeaderSectionLimit::MINIMUM,
                transport: ListenerTransport::Plaintext,
            },
        ));
        Ok(Fixture { address, shutdown, server })
    }

    /// Stop the server and wait for its drain.
    async fn stop(self) -> TestResult {
        self.shutdown.send(()).map_err(|()| "the server stopped early")?;
        self.server.await??;
        Ok(())
    }
}

/// Everything `stream` receives until the broker closes it.
async fn received_until_closed(stream: &mut TcpStream) -> Result<String, Box<dyn Error + Send + Sync>> {
    let mut received = Vec::new();
    stream.read_to_end(&mut received).await?;
    Ok(String::from_utf8_lossy(&received).into_owned())
}

#[tokio::test]
async fn a_half_request_head_is_answered_408_and_closed_after_the_head_bound() -> TestResult {
    let fixture = Fixture::start().await?;
    // Taken before the connection opens, since the server counts from its opening.
    let opening = Instant::now();
    let mut stream = TcpStream::connect(fixture.address).await?;
    stream.write_all(HALF_HEAD).await?;

    let answer = received_until_closed(&mut stream).await?;
    let waited = opening.elapsed();

    assert!(answer.starts_with("HTTP/1.1 408 Request Timeout\r\n"), "{answer}");
    assert!(answer.contains("\r\nconnection: close\r\n"), "{answer}");
    assert!(waited >= BOUNDS.head, "closed after {waited:?}");
    assert!(waited < BOUNDS.idle, "closed after {waited:?}");
    fixture.stop().await
}

#[tokio::test]
async fn a_connection_that_sends_nothing_is_closed_after_the_head_bound_without_an_answer() -> TestResult {
    let fixture = Fixture::start().await?;
    let opening = Instant::now();
    let mut stream = TcpStream::connect(fixture.address).await?;

    let answer = received_until_closed(&mut stream).await?;
    let waited = opening.elapsed();

    pretty_assertions::assert_eq!(answer, "");
    assert!(waited >= BOUNDS.head, "closed after {waited:?}");
    assert!(waited < BOUNDS.idle, "closed after {waited:?}");
    fixture.stop().await
}

#[tokio::test]
async fn a_head_arriving_in_parts_inside_the_head_bound_is_served() -> TestResult {
    let fixture = Fixture::start().await?;
    let mut stream = TcpStream::connect(fixture.address).await?;
    stream.write_all(HALF_HEAD).await?;
    sleep(BOUNDS.head / 2).await;
    stream.write_all(b"Connection: close\r\n\r\n").await?;

    let answer = received_until_closed(&mut stream).await?;

    assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
    assert!(answer.ends_with("served"), "{answer}");
    fixture.stop().await
}

#[tokio::test]
async fn an_idle_keep_alive_connection_is_closed_after_the_idle_bound() -> TestResult {
    let fixture = Fixture::start().await?;
    let mut stream = TcpStream::connect(fixture.address).await?;
    let sent = Instant::now();
    stream.write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n").await?;
    let mut response = Vec::new();
    while !response.ends_with(b"served") {
        let mut chunk = [0; 512];
        let read = stream.read(&mut chunk).await?;
        assert_ne!(read, 0, "closed before the response ended");
        response.extend_from_slice(chunk.get(..read).unwrap_or_default());
    }

    let rest = received_until_closed(&mut stream).await?;

    pretty_assertions::assert_eq!(rest, "");
    assert!(sent.elapsed() >= BOUNDS.idle, "closed {:?} after the request", sent.elapsed());
    fixture.stop().await
}

#[tokio::test]
async fn a_request_in_flight_past_the_idle_bound_is_answered() -> TestResult {
    let fixture = Fixture::start().await?;
    let response = reqwest::get(format!("http://{}/slow", fixture.address)).await?;

    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(response.text().await?, "slow");
    fixture.stop().await
}

#[tokio::test]
async fn an_idle_http2_connection_is_closed_gracefully_after_the_idle_bound() -> TestResult {
    let fixture = Fixture::start().await?;
    let stream = TcpStream::connect(fixture.address).await?;
    let (mut sender, connection) = handshake(TokioExecutor::new(), TokioIo::new(stream)).await?;
    let connection = tokio::spawn(connection);
    let sent = Instant::now();
    let response = sender
        .send_request(hyper::Request::get(format!("http://{}/test", fixture.address)).body(Body::empty())?)
        .await?;
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    drop(response);

    connection.await??;

    assert!(sent.elapsed() >= BOUNDS.idle, "closed {:?} after the request", sent.elapsed());
    fixture.stop().await
}
