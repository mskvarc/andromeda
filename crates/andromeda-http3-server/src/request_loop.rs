//! Accepting QUIC connections, then the requests on each, and dispatching
//! every request to the router.
//!
//! Every request carries the address of the peer its connection came from as
//! a `ConnectInfo<SocketAddr>` extension, as on the TCP listener, so the edge
//! decides whose forwarded fields to believe alike on both.
//!
//! Each connection is set up with the request header section limit as its
//! `SETTINGS_MAX_FIELD_SECTION_SIZE` (RFC 9114 section 4.2.2). A client that
//! has read the setting does not send a request over it (RFC 9114 section
//! 7.2.4.2); one that sends it anyway is answered 431 before the request
//! reaches the router.

use andromeda_configuration::header_section_limit::HeaderSectionLimit;
use andromeda_http_server::close_policy::ClosePolicy;
use axum::{
    Router,
    body::{Body, Bytes},
    extract::ConnectInfo,
    http::{Request, Response},
};
use h3::server::{self, RequestResolver};
use h3_quinn::{BidiStream, Connection};
use h3_util::server_body::{H3IncomingServer, send_h3_server_body};
use quinn::{Endpoint, Incoming};
use std::{future::Future, net::SocketAddr, time::Duration};
use tokio::{sync::watch, task::JoinSet, time::timeout};
use tower::ServiceExt;

/// How a drain after shutdown ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drain {
    /// Every connection finished before the drain deadline.
    Complete,
    /// Connections were still open at the drain deadline and were cut off.
    DeadlineExceeded,
}

/// Accept connections on `endpoint` and serve each on its own task, its
/// handshake included, until `shutdown` resolves or the endpoint closes, then
/// drain them.
///
/// Draining announces the close on every open connection with a GOAWAY frame
/// (RFC 9114 section 5.2), lets the requests already accepted finish, and
/// cuts off whatever is still open once `policy`'s drain deadline has passed.
pub(crate) async fn accept_connections<F>(endpoint: &Endpoint, router: Router, limit: HeaderSectionLimit, policy: ClosePolicy, shutdown: F) -> Drain
where
    F: Future<Output = ()>,
{
    let (draining, drain_signal) = watch::channel(());
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        let accepted = tokio::select! {
            accepted = endpoint.accept() => accepted,
            () = &mut shutdown => break,
            Some(_finished) = connections.join_next(), if !connections.is_empty() => continue,
        };
        let Some(incoming) = accepted else {
            break;
        };
        connections.spawn(serve_connection(
            incoming,
            router.clone(),
            limit,
            drain_signal.clone(),
            policy.announced_close_delay,
        ));
    }
    draining.send_replace(());
    let finished = timeout(policy.drain_deadline, async { while connections.join_next().await.is_some() {} }).await;
    match finished {
        Ok(()) => Drain::Complete,
        Err(_elapsed) => {
            connections.abort_all();
            Drain::DeadlineExceeded
        }
    }
}

/// Complete the handshake of `incoming`, then serve every request the
/// connection carries, each on its own task.
///
/// When `draining` fires the connection announces its close and finishes its
/// accepted requests. Once none is left it waits up to `linger` for the peer
/// to close first, so a response already written is not cut off by a close
/// that outruns it.
async fn serve_connection(incoming: Incoming, router: Router, limit: HeaderSectionLimit, mut draining: watch::Receiver<()>, linger: Duration) {
    let peer = incoming.remote_address();
    let established = match incoming.await {
        Ok(established) => established,
        Err(error) => {
            tracing::debug!(peer = %peer, error = %error, "QUIC handshake failed");
            return;
        }
    };
    let mut connection = match server::builder()
        .max_field_section_size(u64::from(limit.octets()))
        .build::<Connection, Bytes>(Connection::new(established))
        .await
    {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(error = %error, "HTTP/3 connection setup failed");
            return;
        }
    };
    let mut requests = JoinSet::new();
    let mut announced = false;
    loop {
        if announced && requests.is_empty() {
            let _closed_by_peer = timeout(linger, wait_for_peer_close(&mut connection)).await;
            return;
        }
        tokio::select! {
            accepted = connection.accept() => match accepted {
                Ok(Some(resolver)) => {
                    requests.spawn(serve_request(resolver, router.clone(), peer));
                }
                Ok(None) => return,
                Err(error) => {
                    if !error.is_h3_no_error() {
                        tracing::warn!(error = %error, "HTTP/3 connection ended with an error");
                    }
                    return;
                }
            },
            Some(_finished) = requests.join_next(), if !requests.is_empty() => {}
            _changed = draining.changed(), if !announced => {
                announced = true;
                if let Err(error) = connection.shutdown(0).await {
                    tracing::warn!(error = %error, "HTTP/3 close announcement failed");
                    return;
                }
            }
        }
    }
}

/// Resolve once the peer has closed `connection` or finished its side of it.
///
/// A connection that announced its close accepts no further request, so
/// anything it yields before the peer closes is dropped.
async fn wait_for_peer_close(connection: &mut server::Connection<Connection, Bytes>) {
    while let Ok(Some(_resolver)) = connection.accept().await {}
}

/// Read one request from `peer`, answer it from `router` and stream the
/// response back.
async fn serve_request(resolver: RequestResolver<Connection, Bytes>, router: Router, peer: SocketAddr) {
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
    let mut request = Request::from_parts(parts, Body::new(H3IncomingServer::new(receiver)));
    request.extensions_mut().insert(ConnectInfo(peer));
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
