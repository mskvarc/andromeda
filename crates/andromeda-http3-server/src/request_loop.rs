//! Accepting QUIC connections, then the requests on each, and dispatching
//! every request to the router.
//!
//! Each connection is set up with the request header section limit as its
//! `SETTINGS_MAX_FIELD_SECTION_SIZE` (RFC 9114 section 4.2.2). A client that
//! has read the setting does not send a request over it (RFC 9114 section
//! 7.2.4.2); one that sends it anyway is answered 431 before the request
//! reaches the router.

use andromeda_configuration_scalars::header_section_limit::HeaderSectionLimit;
use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, Response},
};
use h3::server::{self, RequestResolver};
use h3_quinn::{BidiStream, Connection};
use h3_util::{
    quinn::H3QuinnAcceptor,
    server::H3Acceptor,
    server_body::{H3IncomingServer, send_h3_server_body},
};
use std::future::Future;
use tower::ServiceExt;

/// Accept connections from `acceptor` and serve each on its own task until
/// `shutdown` resolves or the endpoint closes.
///
/// # Errors
///
/// Returns the acceptor's type-erased failure when accepting fails.
pub(crate) async fn accept_connections<F>(mut acceptor: H3QuinnAcceptor, router: Router, limit: HeaderSectionLimit, shutdown: F) -> Result<(), h3_util::Error>
where
    F: Future<Output = ()>,
{
    tokio::pin!(shutdown);
    loop {
        let accepted = tokio::select! {
            accepted = acceptor.accept() => accepted?,
            () = &mut shutdown => return Ok(()),
        };
        let Some(connection) = accepted else {
            return Ok(());
        };
        // clone: every connection dispatches to the one shared router.
        tokio::spawn(serve_connection(connection, router.clone(), limit));
    }
}

/// Serve every request `connection` carries, each on its own task.
async fn serve_connection(connection: Connection, router: Router, limit: HeaderSectionLimit) {
    let mut connection = match server::builder()
        .max_field_section_size(u64::from(limit.octets()))
        .build::<Connection, Bytes>(connection)
        .await
    {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(error = %error, "HTTP/3 connection setup failed");
            return;
        }
    };
    loop {
        match connection.accept().await {
            Ok(Some(resolver)) => {
                // clone: every request dispatches to the one shared router.
                tokio::spawn(serve_request(resolver, router.clone()));
            }
            Ok(None) => break,
            Err(error) => {
                if !error.is_h3_no_error() {
                    tracing::warn!(error = %error, "HTTP/3 connection ended with an error");
                }
                break;
            }
        }
    }
}

/// Read one request, answer it from `router` and stream the response back.
async fn serve_request(resolver: RequestResolver<Connection, Bytes>, router: Router) {
    // A section over the limit is answered 431 while resolving, and ends here.
    let (request, stream) = match resolver.resolve_request().await {
        Ok(request_and_stream) => request_and_stream,
        Err(error) => {
            tracing::debug!(error = %error, "HTTP/3 request refused before dispatch");
            return;
        }
    };
    let (mut sender, receiver) = stream.split();
    let (parts, ()) = request.into_parts();
    let request = Request::from_parts(parts, Body::new(H3IncomingServer::new(receiver)));
    let response = match router.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    let (parts, body) = response.into_parts();
    if let Err(error) = sender.send_response(Response::from_parts(parts, ())).await {
        tracing::warn!(error = %error, "HTTP/3 response head could not be sent");
        return;
    }
    if let Err(error) = send_h3_server_body::<Body, BidiStream<Bytes>>(&mut sender, body).await {
        tracing::warn!(error = %error, "HTTP/3 response body could not be sent");
    }
}
