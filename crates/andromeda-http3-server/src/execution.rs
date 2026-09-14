//! Serving an assembled axum router over an HTTP/3 (QUIC) endpoint.
//!
//! The bound endpoint is driven by [`axum_h3::H3Router`], which accepts each
//! QUIC connection, then each request on it, and dispatches the request to the
//! same axum router the TCP listener serves. Serving ends when the supplied
//! shutdown future resolves, after which in-flight connections are given until
//! the endpoint goes idle to finish.

use axum::Router;
use axum_h3::H3Router;
use h3_util::quinn::H3QuinnAcceptor;
use quinn::Endpoint;
use std::{future::Future, io};
use thiserror::Error;

/// Failure while serving HTTP/3.
#[derive(Debug, Error)]
pub enum Http3ServeError {
    /// The bound endpoint's local address could not be read.
    #[error("local address of the QUIC endpoint is unavailable: {0}")]
    LocalAddress(#[source] io::Error),
    /// The HTTP/3 connection accept loop terminated with an error.
    ///
    /// The underlying `h3` failure is type-erased upstream, so its human-readable
    /// rendering is captured here rather than the opaque error itself.
    #[error("HTTP/3 serving failed: {message}")]
    Serve {
        /// The rendered cause of the serving failure.
        message: String,
    },
}

/// Serve `router` over the bound QUIC `endpoint` until `shutdown` resolves.
///
/// After the shutdown future resolves the accept loop stops taking new
/// connections and the endpoint is awaited idle, letting connections already in
/// flight complete before the function returns.
///
/// # Errors
///
/// Returns [`Http3ServeError::LocalAddress`] when the endpoint's address cannot
/// be read for logging, and [`Http3ServeError::Serve`] when the accept loop
/// fails.
pub async fn serve_endpoint<F>(endpoint: Endpoint, router: Router, shutdown: F) -> Result<(), Http3ServeError>
where
    F: Future<Output = ()> + Send,
{
    let local_address = endpoint.local_addr().map_err(Http3ServeError::LocalAddress)?;
    tracing::info!(address = %local_address, "HTTP/3 listener accepting connections");
    // The acceptor takes ownership of the endpoint handle; a second cheap,
    // Arc-backed handle is retained so the endpoint can be awaited idle once
    // serving stops.
    let acceptor = H3QuinnAcceptor::new(endpoint.clone());
    let result = H3Router::from(router).serve_with_shutdown(acceptor, shutdown).await;
    endpoint.wait_idle().await;
    result.map_err(|error| Http3ServeError::Serve { message: error.to_string() })
}

#[cfg(test)]
mod tests {
    use crate::{endpoint::bind, execution::serve_endpoint, test_support::SelfSignedListener};
    use axum::{Router, routing::get};
    use bytes::Buf;
    use h3::client;
    use h3_quinn::Connection;
    use http::StatusCode;
    use quinn::{Endpoint, crypto::rustls::QuicClientConfig};
    use rustls::{RootCertStore, crypto::aws_lc_rs::default_provider};
    use std::{
        error::Error,
        future::poll_fn,
        net::{Ipv4Addr, SocketAddr},
        sync::Arc,
    };
    use tokio::sync::oneshot;

    /// A response read back over an HTTP/3 request stream.
    struct Http3Response {
        status: StatusCode,
        body: String,
    }

    /// Dial the listener over HTTP/3, trusting only its self-signed certificate,
    /// and read one GET response in full.
    async fn http3_get(listener: &SelfSignedListener, server: SocketAddr, path: &str) -> Result<Http3Response, Box<dyn Error>> {
        let mut roots = RootCertStore::empty();
        roots.add(listener.certificate())?;
        let mut client_crypto = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        client_crypto.alpn_protocols = vec![b"h3".to_vec()];
        let client_config = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));
        let mut client_endpoint = Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        client_endpoint.set_default_client_config(client_config);

        let connection = client_endpoint.connect(server, "localhost")?.await?;
        let (mut driver, mut send_request) = client::new(Connection::new(connection)).await?;
        let drive = tokio::spawn(async move {
            // Resolves with the closing error once the connection ends; that is
            // the expected terminal state, not a failure.
            let _closed = poll_fn(|cx| driver.poll_close(cx)).await;
        });

        let request = http::Request::get(format!("https://localhost:{}{path}", server.port())).body(())?;
        let mut stream = send_request.send_request(request).await?;
        stream.finish().await?;
        let response = stream.recv_response().await?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await? {
            while chunk.has_remaining() {
                let slice = chunk.chunk();
                body.extend_from_slice(slice);
                let consumed = slice.len();
                chunk.advance(consumed);
            }
        }
        drop(send_request);
        drive.await?;
        client_endpoint.wait_idle().await;
        Ok(Http3Response {
            status,
            body: String::from_utf8(body)?,
        })
    }

    #[tokio::test]
    async fn a_get_is_served_over_http3() -> Result<(), Box<dyn Error>> {
        let listener = SelfSignedListener::generate()?;
        let activation = listener.config().activation()?.expect("enabled config yields activation");
        let endpoint = bind(activation, SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let server_address = endpoint.local_addr()?;
        let router = Router::new().route("/probe", get(|| async { "h3 works" }));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_endpoint(endpoint, router, async move {
            let _ = shutdown_receiver.await;
        }));

        let response = http3_get(&listener, server_address, "/probe").await?;
        pretty_assertions::assert_eq!(response.status, StatusCode::OK);
        pretty_assertions::assert_eq!(response.body, "h3 works");

        let _ = shutdown_sender.send(());
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn a_status_only_response_is_served_over_http3() -> Result<(), Box<dyn Error>> {
        let listener = SelfSignedListener::generate()?;
        let activation = listener.config().activation()?.expect("enabled config yields activation");
        let endpoint = bind(activation, SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let server_address = endpoint.local_addr()?;
        let router = Router::new().route("/echo", get(|| async { StatusCode::NO_CONTENT }));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(serve_endpoint(endpoint, router, async move {
            let _ = shutdown_receiver.await;
        }));

        let response = http3_get(&listener, server_address, "/echo").await?;
        pretty_assertions::assert_eq!(response.status, StatusCode::NO_CONTENT);

        let _ = shutdown_sender.send(());
        server.await??;
        Ok(())
    }
}
