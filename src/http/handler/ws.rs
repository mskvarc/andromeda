//! WebSocket stream handler for subscription notifications.
//!
//! Provides a `GET /subscriptions/{subscriptionId}/ws` endpoint that
//! upgrades to a persistent WebSocket connection for receiving real-time
//! notifications from a WebSocket-transport subscription.

use std::{sync::Arc, time::Duration};

use andromeda_ngsi_ld_lexicon::uri::Uri;
use axum::{
    extract::{
        OriginalUri,
        Path,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace};

use crate::{
    api::tenant::TenantState,
    core::error::{AndromedaError, ResultInstanceExt},
    http::error::problem::NgsiLdError,
};
use andromeda_delivery::{
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
    stream::{
        registry::{ConnectionId, ConnectionRegistry},
        ws::WsNotificationMessage,
    },
};

/// RAII guard that unregisters a WebSocket connection when the stream ends.
struct WsConnectionGuard {
    subscription_id: Uri,
    connection_id: ConnectionId,
    registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
}

impl Drop for WsConnectionGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.subscription_id, self.connection_id);
    }
}

/// `GET /ngsi-ld/v1/subscriptions/{subscriptionId}/ws` -- Upgrade to a WebSocket notification stream.
///
/// Validates that the subscription exists and uses WebSocket transport, then
/// upgrades the HTTP connection to a WebSocket that streams notification
/// messages as they are dispatched by the notification worker.
pub async fn stream(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(subscription_id): Path<String>,
    ws: axum::extract::WebSocketUpgrade,
) -> Result<Response, NgsiLdError> {
    let subscription_id: Uri = subscription_id
        .parse()
        .map_err(|_| AndromedaError::malformed_request(format!("Invalid subscription id: {subscription_id}")))
        .with_instance(&uri)?;
    // Validate subscription exists.
    let subscription = state.subscriptions.get(subscription_id.as_str()).await.with_instance(&uri)?;

    // Validate subscription uses WebSocket transport via the canonical parser.
    let is_ws = parse_endpoint_transport(
        subscription.notification.endpoint.uri.as_str(),
        subscription.notification.endpoint.notifier_info.as_deref(),
    )
    .is_ok_and(|t| matches!(t, EndpointTransport::WebSocket));

    if !is_ws {
        return Err(NgsiLdError::from(
            AndromedaError::subscription_validation(
                "Subscription endpoint is not a WebSocket transport. Only subscriptions with ws:// endpoint URI support WebSocket streaming.",
            )
            .with_instance(&uri),
        ));
    }

    // Get WS registry and config.
    let ws_registry = Arc::clone(&state.ws_registry);
    let ping_interval_secs = state.ws_ping_interval_secs;
    let buffer_size = state.ws_channel_buffer_size;

    // Register connection.
    let (connection_id, rx, cancellation_token) = ws_registry.register(&subscription_id, buffer_size);

    debug!(%subscription_id, ?connection_id, "WebSocket client connected");

    let guard = WsConnectionGuard {
        subscription_id,
        connection_id,
        registry: ws_registry,
    };

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, rx, cancellation_token, guard, ping_interval_secs)))
}

/// Drive the WebSocket connection after upgrade.
///
/// Runs two concurrent loops:
/// - Outbound: forwards notification messages from the mpsc channel as text
///   frames, sends periodic pings, and watches the cancellation token.
/// - Inbound: reads client frames; close/error breaks the loop, text/binary
///   and pong are logged at trace level and ignored.
async fn handle_ws(
    socket: WebSocket,
    mut rx: mpsc::Receiver<WsNotificationMessage>,
    cancellation_token: CancellationToken,
    guard: WsConnectionGuard,
    ping_interval_secs: u64,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut ping_interval = tokio::time::interval(Duration::from_secs(ping_interval_secs));
    // First tick completes immediately; skip it so the first ping fires after the interval.
    ping_interval.tick().await;

    loop {
        tokio::select! {
            // Outbound: notification from registry.
            msg = rx.recv() => {
                match msg {
                    Some(notification) => {
                        if sender.send(Message::Text(notification.data.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            // Cancellation: subscription deleted or transport changed.
            _ = cancellation_token.cancelled() => {
                let _ = sender.send(Message::Close(None)).await;
                break;
            }
            // Keep-alive ping.
            _ = ping_interval.tick() => {
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            // Inbound: client frames.
            frame = receiver.next() => {
                match frame {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Pong(_))) => {
                        trace!("WebSocket pong received");
                    }
                    Some(Ok(Message::Text(_))) => {
                        trace!("WebSocket text frame from client ignored");
                    }
                    Some(Ok(Message::Binary(_))) => {
                        trace!("WebSocket binary frame from client ignored");
                    }
                    Some(Ok(Message::Ping(_))) => {
                        // tungstenite auto-replies with Pong
                    }
                    Some(Err(_)) => break,
                }
            }
        }
    }

    debug!(
        subscription_id = %guard.subscription_id,
        connection_id = ?guard.connection_id,
        "WebSocket client disconnected"
    );
    // guard drops here -> unregister
}
