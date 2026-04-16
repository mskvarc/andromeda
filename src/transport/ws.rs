//! WebSocket notification transport.
//!
//! Provides a connection registry that bridges the notification delivery
//! pipeline with persistent WebSocket client connections. Each WebSocket
//! subscription supports multiple concurrent client connections, each
//! identified by a unique [`ConnectionId`]. Messages are fanned out to all
//! connected clients.

use crate::transport::types::ConnectionId;
use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// A single WebSocket notification message to be sent to a connected client.
#[derive(Debug, Clone)]
pub struct WsNotificationMessage {
    /// JSON-serialized notification payload, sent as a text frame.
    pub data: String,
}

/// Errors from attempting to send a WebSocket notification message.
#[derive(Debug, thiserror::Error)]
pub enum WsSendError {
    /// No WebSocket client is currently connected for the given subscription.
    #[error("no WebSocket client connected for subscription {subscription_id}")]
    NoClient { subscription_id: String },
    /// The WebSocket channel buffer is full for all connected clients.
    #[error("WebSocket channel buffer full for all clients on subscription {subscription_id}")]
    BufferFull { subscription_id: String },
}

/// Thread-safe registry mapping subscription IDs to active WebSocket client connections.
///
/// Each subscription supports multiple concurrent WebSocket clients. Messages
/// are fanned out to all connected clients for a given subscription. Stale
/// connections are cleaned up during send operations.
#[derive(Debug)]
pub struct WsConnectionRegistry {
    connections: DashMap<String, DashMap<ConnectionId, (mpsc::Sender<WsNotificationMessage>, CancellationToken)>>,
}

impl WsConnectionRegistry {
    /// Create an empty connection registry.
    pub fn new() -> WsConnectionRegistry {
        WsConnectionRegistry { connections: DashMap::new() }
    }

    /// Register a new WebSocket client connection for a subscription.
    ///
    /// Returns the connection identifier, the receiver end of the notification
    /// channel, and a cancellation token that signals when the subscription is
    /// deleted. Multiple clients can coexist on the same subscription.
    pub fn register(&self, subscription_id: &str, buffer_size: usize) -> (ConnectionId, mpsc::Receiver<WsNotificationMessage>, CancellationToken) {
        let conn_id = ConnectionId::next();
        let (tx, rx) = mpsc::channel(buffer_size);
        let token = CancellationToken::new();
        self.connections
            .entry(subscription_id.to_string())
            .or_insert_with(DashMap::new)
            .insert(conn_id, (tx, token.clone())); // clone: CancellationToken is Arc-based
        (conn_id, rx, token)
    }

    /// Remove a specific client connection for a subscription.
    ///
    /// Called by the `WsConnectionGuard` on client disconnect. Only removes
    /// the targeted connection; other clients on the same subscription are
    /// unaffected. Cleans up the outer entry when no connections remain.
    pub fn unregister(&self, subscription_id: &str, connection_id: ConnectionId) {
        if let Some(inner) = self.connections.get(subscription_id) {
            inner.remove(&connection_id);
        }
        // Remove outer entry if inner map is now empty.
        self.connections.remove_if(subscription_id, |_, inner| inner.is_empty());
    }

    /// Attempt to send a notification message to all connected WebSocket clients.
    ///
    /// Fan-out: the message is cloned and sent to every active client for the
    /// subscription. Closed channels are removed during iteration. Returns
    /// `Ok(())` if at least one client received the message. Returns
    /// `BufferFull` only if all clients have full buffers.
    pub fn try_send(&self, subscription_id: &str, message: WsNotificationMessage) -> Result<(), WsSendError> {
        let inner = self.connections.get(subscription_id).ok_or_else(|| WsSendError::NoClient {
            subscription_id: subscription_id.to_string(),
        })?;

        let mut any_sent = false;
        let mut any_full = false;
        let mut closed_ids = Vec::new();

        for entry in inner.iter() {
            let (sender, _) = entry.value();
            match sender.try_send(message.clone()) {
                // clone: each client gets its own copy
                Ok(()) => {
                    any_sent = true;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    any_full = true;
                    debug!(
                        subscription_id,
                        connection_id = ?entry.key(),
                        "WebSocket channel buffer full for client, skipping"
                    );
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    closed_ids.push(*entry.key());
                }
            }
        }

        // Clean up closed connections.
        for id in &closed_ids {
            inner.remove(id);
        }
        // Drop the inner ref before potentially removing the outer entry.
        let remaining = inner.len();
        drop(inner);

        if !closed_ids.is_empty() && remaining == 0 {
            self.connections.remove_if(subscription_id, |_, m| m.is_empty());
        }

        if any_sent {
            Ok(())
        } else if any_full {
            // All live clients had full buffers (some may have been closed
            // and cleaned up, but at least one live client exists).
            Err(WsSendError::BufferFull {
                subscription_id: subscription_id.to_string(),
            })
        } else {
            Err(WsSendError::NoClient {
                subscription_id: subscription_id.to_string(),
            })
        }
    }

    /// Close all WebSocket connections for a subscription.
    ///
    /// Cancels every cancellation token (signalling WebSocket streams to end)
    /// and removes all connection entries.
    pub fn close(&self, subscription_id: &str) {
        if let Some((_, inner)) = self.connections.remove(subscription_id) {
            for entry in inner.into_iter() {
                let (_, (_, token)) = entry;
                token.cancel();
            }
        }
    }
}

#[cfg(test)]
impl WsConnectionRegistry {
    /// Check whether any client is currently connected for a subscription.
    pub fn is_connected(&self, subscription_id: &str) -> bool {
        self.connections.get(subscription_id).map_or(false, |inner| !inner.is_empty())
    }

    /// Count active connections for a subscription.
    pub fn connection_count(&self, subscription_id: &str) -> usize {
        self.connections.get(subscription_id).map_or(0, |inner| inner.len())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn register_returns_unique_connection_ids() {
        let registry = WsConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        let (conn_b, _rx_b, _token_b) = registry.register("sub1", 8);
        assert_ne!(conn_a, conn_b);
    }

    #[tokio::test]
    async fn register_returns_working_receiver() {
        let registry = WsConnectionRegistry::new();
        let (_, mut rx, _token) = registry.register("sub1", 8);

        let message = WsNotificationMessage {
            data: r#"{"id":"notif1"}"#.to_string(),
        };
        registry.try_send("sub1", message).unwrap();

        let received = rx.recv().await.unwrap();
        pretty_assertions::assert_eq!(received.data, r#"{"id":"notif1"}"#);
    }

    #[tokio::test]
    async fn fanout_delivers_to_all_clients() {
        let registry = WsConnectionRegistry::new();
        let (_, mut rx_a, _token_a) = registry.register("sub1", 8);
        let (_, mut rx_b, _token_b) = registry.register("sub1", 8);

        let message = WsNotificationMessage { data: "{}".to_string() };
        registry.try_send("sub1", message).unwrap();

        let received_a = rx_a.recv().await.unwrap();
        let received_b = rx_b.recv().await.unwrap();
        pretty_assertions::assert_eq!(received_a.data, "{}");
        pretty_assertions::assert_eq!(received_b.data, "{}");
    }

    #[tokio::test]
    async fn unregister_removes_only_target_connection() {
        let registry = WsConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        let (_, mut rx_b, _token_b) = registry.register("sub1", 8);

        registry.unregister("sub1", conn_a);
        assert_eq!(registry.connection_count("sub1"), 1);

        let message = WsNotificationMessage { data: "{}".to_string() };
        registry.try_send("sub1", message).unwrap();

        let received = rx_b.recv().await.unwrap();
        pretty_assertions::assert_eq!(received.data, "{}");
    }

    #[tokio::test]
    async fn stale_unregister_does_not_affect_other_connections() {
        let registry = WsConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        let (_, mut rx_b, _token_b) = registry.register("sub1", 8);

        // Unregister stale connection A; B must still work.
        registry.unregister("sub1", conn_a);

        let message = WsNotificationMessage { data: "{}".to_string() };
        registry.try_send("sub1", message).unwrap();

        let received = rx_b.recv().await.unwrap();
        pretty_assertions::assert_eq!(received.data, "{}");
    }

    #[tokio::test]
    async fn close_terminates_all_connections() {
        let registry = WsConnectionRegistry::new();
        let (_, mut rx_a, token_a) = registry.register("sub1", 8);
        let (_, mut rx_b, token_b) = registry.register("sub1", 8);

        registry.close("sub1");

        assert!(token_a.is_cancelled());
        assert!(token_b.is_cancelled());
        assert!(rx_a.recv().await.is_none());
        assert!(rx_b.recv().await.is_none());
        assert!(!registry.is_connected("sub1"));
    }

    #[tokio::test]
    async fn try_send_no_client_after_all_unregistered() {
        let registry = WsConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        registry.unregister("sub1", conn_a);

        let message = WsNotificationMessage { data: "{}".to_string() };
        assert!(matches!(registry.try_send("sub1", message), Err(WsSendError::NoClient { .. })));
    }

    #[tokio::test]
    async fn try_send_no_client_returns_error() {
        let registry = WsConnectionRegistry::new();
        let message = WsNotificationMessage { data: "{}".to_string() };
        let result = registry.try_send("nonexistent", message);
        assert!(matches!(result, Err(WsSendError::NoClient { .. })));
    }

    #[tokio::test]
    async fn try_send_buffer_full_returns_error() {
        let registry = WsConnectionRegistry::new();
        let (_conn, _rx, _token) = registry.register("sub1", 1);

        let msg1 = WsNotificationMessage { data: "{}".to_string() };
        registry.try_send("sub1", msg1).unwrap();

        let msg2 = WsNotificationMessage { data: "{}".to_string() };
        let result = registry.try_send("sub1", msg2);
        assert!(matches!(result, Err(WsSendError::BufferFull { .. })));
    }

    #[tokio::test]
    async fn unregister_removes_connection() {
        let registry = WsConnectionRegistry::new();
        let (conn_id, _rx, _token) = registry.register("sub1", 8);
        assert!(registry.is_connected("sub1"));

        registry.unregister("sub1", conn_id);
        assert!(!registry.is_connected("sub1"));

        let message = WsNotificationMessage { data: "{}".to_string() };
        assert!(matches!(registry.try_send("sub1", message), Err(WsSendError::NoClient { .. })));
    }

    #[tokio::test]
    async fn is_connected_reflects_state() {
        let registry = WsConnectionRegistry::new();
        assert!(!registry.is_connected("sub1"));

        let (conn_id, _rx, _token) = registry.register("sub1", 8);
        assert!(registry.is_connected("sub1"));

        registry.unregister("sub1", conn_id);
        assert!(!registry.is_connected("sub1"));
    }

    #[tokio::test]
    async fn try_send_mixed_full_and_closed_returns_buffer_full() {
        let registry = WsConnectionRegistry::new();

        // Client A: buffer size 1, fill it so next send is Full.
        let (_conn_a, _rx_a, _token_a) = registry.register("sub1", 1);
        let fill = WsNotificationMessage { data: "{}".to_string() };
        registry.try_send("sub1", fill).unwrap();

        // Client B: drop the receiver so channel is Closed.
        let (_conn_b, rx_b, _token_b) = registry.register("sub1", 1);
        drop(rx_b);

        // Send again: A is Full, B is Closed. Should be BufferFull, not NoClient.
        let message = WsNotificationMessage { data: "{}".to_string() };
        let result = registry.try_send("sub1", message);
        assert!(matches!(result, Err(WsSendError::BufferFull { .. })));
    }
}
