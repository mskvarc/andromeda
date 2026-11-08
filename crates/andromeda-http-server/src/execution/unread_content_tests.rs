//! Refusals answered before an upload is read, over real sockets: each
//! announces the close, and the client can keep sending until the
//! announced-close delay has passed.

use crate::{
    close_policy::ClosePolicy,
    connection_bounds::ConnectionBounds,
    error::HttpServerError,
    execution::serve_listener,
    field_section::HeaderSectionPolicy,
};
use andromeda_configuration::{header_section_limit::HeaderSectionLimit, schema::listener_transport::ListenerTransport};
use axum::{Router, http::StatusCode, routing::post};
use std::{error::Error, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpListener,
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::oneshot,
    task::JoinHandle,
    time::{Instant, timeout},
};

/// What a fixture test returns.
type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

/// The close policy every fixture server runs under.
const POLICY: ClosePolicy = ClosePolicy {
    drain_deadline: Duration::from_secs(5),
    announced_close_delay: Duration::from_millis(500),
};

/// The content length every upload declares: 32 MiB, far more than the
/// socket buffers hold while the server reads nothing.
const DECLARED: usize = 32 * 1024 * 1024;

/// The octets one write of an upload carries.
const CHUNK: usize = 64 * 1024;

/// The longest a test waits for the server to end an upload.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// Serve a router answering a `POST` of `/entities` with `411` and of
/// `/typed` with `415`, both without reading the content, and `404` to
/// anything else.
async fn serve() -> Result<(SocketAddr, oneshot::Sender<()>, JoinHandle<Result<(), HttpServerError>>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = Router::new()
        .route("/entities", post(|| async { StatusCode::LENGTH_REQUIRED }))
        .route("/typed", post(|| async { StatusCode::UNSUPPORTED_MEDIA_TYPE }));
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
        ConnectionBounds {
            head: Duration::from_secs(30),
            idle: Duration::from_secs(180),
        },
        HeaderSectionPolicy {
            limit: HeaderSectionLimit::MINIMUM,
            transport: ListenerTransport::Plaintext,
        },
    ));
    Ok((address, shutdown, server))
}

/// How the content of an upload is framed.
#[derive(Clone, Copy, Debug)]
enum Framing {
    /// `Content-Length` declares [`DECLARED`] octets.
    Declared,
    /// `Transfer-Encoding: chunked`, sending chunks until the connection goes.
    Chunked,
}

/// Write the head of an upload to `path` and then its content, until the
/// connection fails; resolves when the first write failed.
async fn upload(mut writer: OwnedWriteHalf, path: &str, framing: Framing) -> Result<Instant, Box<dyn Error + Send + Sync>> {
    let length = match framing {
        Framing::Declared => format!("Content-Length: {DECLARED}"),
        Framing::Chunked => "Transfer-Encoding: chunked".to_owned(),
    };
    let head = format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain\r\n{length}\r\n\r\n");
    writer.write_all(head.as_bytes()).await?;
    let data = vec![b' '; CHUNK];
    let chunk = match framing {
        Framing::Declared => data,
        Framing::Chunked => [format!("{CHUNK:x}\r\n").into_bytes(), data, b"\r\n".to_vec()].concat(),
    };
    for _ in 0..DECLARED / CHUNK {
        if writer.write_all(&chunk).await.is_err() {
            return Ok(Instant::now());
        }
    }
    Err("the whole upload was written without the connection failing".into())
}

/// The head of the first response `reader` receives, and when it arrived.
async fn response_head(reader: &mut OwnedReadHalf) -> Result<(String, Instant), Box<dyn Error + Send + Sync>> {
    let mut received = Vec::new();
    while !received.windows(4).any(|window| window == b"\r\n\r\n") {
        let mut chunk = [0; 1024];
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Err("the connection closed before the response head".into());
        }
        received.extend_from_slice(chunk.get(..read).unwrap_or_default());
    }
    Ok((String::from_utf8_lossy(&received).to_ascii_lowercase(), Instant::now()))
}

/// Upload to `path` as `framing` says, and report the response head and how
/// long after it the upload could go on.
async fn refused_upload(path: &str, framing: Framing) -> Result<(String, Duration), Box<dyn Error + Send + Sync>> {
    let (address, shutdown, server) = serve().await?;
    let (mut reader, writer) = TcpStream::connect(address).await?.into_split();
    let path = path.to_owned();
    let uploading = tokio::spawn(async move { upload(writer, &path, framing).await });
    let (head, answered) = timeout(UPLOAD_TIMEOUT, response_head(&mut reader)).await??;
    let failed = timeout(UPLOAD_TIMEOUT, uploading).await???;
    shutdown.send(()).map_err(|()| "the server stopped early")?;
    server.await??;
    Ok((head, failed.saturating_duration_since(answered)))
}

/// Assert that `head` is a `status` announcing the close, and that the
/// upload went on for the announced-close delay after it.
fn assert_announced_close(head: &str, uploading: Duration, status: StatusCode) {
    assert!(head.starts_with(&format!("http/1.1 {}", status.as_u16())), "{head}");
    assert!(head.contains("\r\nconnection: close\r\n"), "{head}");
    assert!(uploading >= POLICY.announced_close_delay, "the upload failed {uploading:?} after the response");
}

#[tokio::test]
async fn a_bare_411_to_a_chunked_upload_announces_the_close_and_lets_the_upload_run_for_the_delay() -> TestResult {
    let (head, uploading) = refused_upload("/entities", Framing::Chunked).await?;
    assert_announced_close(&head, uploading, StatusCode::LENGTH_REQUIRED);
    Ok(())
}

#[tokio::test]
async fn a_415_to_a_declared_upload_announces_the_close_and_lets_the_upload_run_for_the_delay() -> TestResult {
    let (head, uploading) = refused_upload("/typed", Framing::Declared).await?;
    assert_announced_close(&head, uploading, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    Ok(())
}

#[tokio::test]
async fn a_404_to_a_declared_upload_announces_the_close_and_lets_the_upload_run_for_the_delay() -> TestResult {
    let (head, uploading) = refused_upload("/nothing-here", Framing::Declared).await?;
    assert_announced_close(&head, uploading, StatusCode::NOT_FOUND);
    Ok(())
}
