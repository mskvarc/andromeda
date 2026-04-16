//! SSE (Server-Sent Events) notification transport.
//!
//! Provides a connection registry that bridges the notification delivery
//! pipeline with persistent SSE client connections. Each SSE subscription
//! supports multiple concurrent client connections, each identified by a
//! unique [`ConnectionId`]. Events are fanned out to all connected clients.

use crate::transport::types::ConnectionId;
use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// A single SSE notification event to be sent to a connected client.
#[derive(Debug, Clone)]
pub struct SseNotificationEvent {
    /// Notification URN used as the SSE `id:` field for Last-Event-ID reconnection.
    pub event_id: String,
    /// JSON-serialized notification payload for the SSE `data:` field.
    pub data: String,
}

/// Errors from attempting to send an SSE notification event.
#[derive(Debug, thiserror::Error)]
pub enum SseSendError {
    /// No SSE client is currently connected for the given subscription.
    #[error("no SSE client connected for subscription {subscription_id}")]
    NoClient { subscription_id: String },
    /// The SSE channel buffer is full for all connected clients.
    #[error("SSE channel buffer full for all clients on subscription {subscription_id}")]
    BufferFull { subscription_id: String },
}

/// Thread-safe registry mapping subscription IDs to active SSE client connections.
///
/// Each subscription supports multiple concurrent SSE clients. Events are
/// fanned out to all connected clients for a given subscription. Stale
/// connections are cleaned up during send operations.
#[derive(Debug)]
pub struct SseConnectionRegistry {
    connections: DashMap<String, DashMap<ConnectionId, (mpsc::Sender<SseNotificationEvent>, CancellationToken)>>,
}

impl SseConnectionRegistry {
    /// Create an empty connection registry.
    pub fn new() -> SseConnectionRegistry {
        SseConnectionRegistry { connections: DashMap::new() }
    }

    /// Register a new SSE client connection for a subscription.
    ///
    /// Returns the connection identifier, the receiver end of the notification
    /// channel, and a cancellation token that signals when the subscription is
    /// deleted. Multiple clients can coexist on the same subscription.
    pub fn register(&self, subscription_id: &str, buffer_size: usize) -> (ConnectionId, mpsc::Receiver<SseNotificationEvent>, CancellationToken) {
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
    /// Called by the `SseConnectionGuard` on client disconnect. Only removes
    /// the targeted connection; other clients on the same subscription are
    /// unaffected. Cleans up the outer entry when no connections remain.
    pub fn unregister(&self, subscription_id: &str, connection_id: ConnectionId) {
        if let Some(inner) = self.connections.get(subscription_id) {
            inner.remove(&connection_id);
        }
        // Remove outer entry if inner map is now empty.
        self.connections.remove_if(subscription_id, |_, inner| inner.is_empty());
    }

    /// Attempt to send a notification event to all connected SSE clients.
    ///
    /// Fan-out: the event is cloned and sent to every active client for the
    /// subscription. Closed channels are removed during iteration. Returns
    /// `Ok(())` if at least one client received the event. Returns
    /// `BufferFull` only if all clients have full buffers.
    pub fn try_send(&self, subscription_id: &str, event: SseNotificationEvent) -> Result<(), SseSendError> {
        let inner = self.connections.get(subscription_id).ok_or_else(|| SseSendError::NoClient {
            subscription_id: subscription_id.to_string(),
        })?;

        let mut any_sent = false;
        let mut any_full = false;
        let mut closed_ids = Vec::new();

        for entry in inner.iter() {
            let (sender, _) = entry.value();
            match sender.try_send(event.clone()) {
                // clone: each client gets its own copy
                Ok(()) => {
                    any_sent = true;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    any_full = true;
                    debug!(
                        subscription_id,
                        connection_id = ?entry.key(),
                        "SSE channel buffer full for client, skipping"
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
            Err(SseSendError::BufferFull {
                subscription_id: subscription_id.to_string(),
            })
        } else {
            Err(SseSendError::NoClient {
                subscription_id: subscription_id.to_string(),
            })
        }
    }

    /// Close all SSE connections for a subscription.
    ///
    /// Cancels every cancellation token (signalling SSE streams to end)
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
impl SseConnectionRegistry {
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
        let registry = SseConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        let (conn_b, _rx_b, _token_b) = registry.register("sub1", 8);
        assert_ne!(conn_a, conn_b);
    }

    #[tokio::test]
    async fn register_returns_working_receiver() {
        let registry = SseConnectionRegistry::new();
        let (_, mut rx, _token) = registry.register("sub1", 8);

        let event = SseNotificationEvent {
            event_id: "notif1".to_string(),
            data: r#"{"id":"notif1"}"#.to_string(),
        };
        registry.try_send("sub1", event).unwrap();

        let received = rx.recv().await.unwrap();
        pretty_assertions::assert_eq!(received.event_id, "notif1");
        pretty_assertions::assert_eq!(received.data, r#"{"id":"notif1"}"#);
    }

    #[tokio::test]
    async fn fanout_delivers_to_all_clients() {
        let registry = SseConnectionRegistry::new();
        let (_, mut rx_a, _token_a) = registry.register("sub1", 8);
        let (_, mut rx_b, _token_b) = registry.register("sub1", 8);

        let event = SseNotificationEvent {
            event_id: "n1".to_string(),
            data: "{}".to_string(),
        };
        registry.try_send("sub1", event).unwrap();

        let received_a = rx_a.recv().await.unwrap();
        let received_b = rx_b.recv().await.unwrap();
        pretty_assertions::assert_eq!(received_a.event_id, "n1");
        pretty_assertions::assert_eq!(received_b.event_id, "n1");
    }

    #[tokio::test]
    async fn unregister_removes_only_target_connection() {
        let registry = SseConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        let (_, mut rx_b, _token_b) = registry.register("sub1", 8);

        registry.unregister("sub1", conn_a);
        assert_eq!(registry.connection_count("sub1"), 1);

        let event = SseNotificationEvent {
            event_id: "n1".to_string(),
            data: "{}".to_string(),
        };
        registry.try_send("sub1", event).unwrap();

        let received = rx_b.recv().await.unwrap();
        pretty_assertions::assert_eq!(received.event_id, "n1");
    }

    #[tokio::test]
    async fn stale_unregister_does_not_affect_other_connections() {
        let registry = SseConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        let (_, mut rx_b, _token_b) = registry.register("sub1", 8);

        // Unregister stale connection A; B must still work.
        registry.unregister("sub1", conn_a);

        let event = SseNotificationEvent {
            event_id: "n1".to_string(),
            data: "{}".to_string(),
        };
        registry.try_send("sub1", event).unwrap();

        let received = rx_b.recv().await.unwrap();
        pretty_assertions::assert_eq!(received.event_id, "n1");
    }

    #[tokio::test]
    async fn close_terminates_all_connections() {
        let registry = SseConnectionRegistry::new();
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
        let registry = SseConnectionRegistry::new();
        let (conn_a, _rx_a, _token_a) = registry.register("sub1", 8);
        registry.unregister("sub1", conn_a);

        let event = SseNotificationEvent {
            event_id: "n1".to_string(),
            data: "{}".to_string(),
        };
        assert!(matches!(registry.try_send("sub1", event), Err(SseSendError::NoClient { .. })));
    }

    #[tokio::test]
    async fn try_send_no_client_returns_error() {
        let registry = SseConnectionRegistry::new();
        let event = SseNotificationEvent {
            event_id: "notif1".to_string(),
            data: "{}".to_string(),
        };
        let result = registry.try_send("nonexistent", event);
        assert!(matches!(result, Err(SseSendError::NoClient { .. })));
    }

    #[tokio::test]
    async fn try_send_buffer_full_returns_error() {
        let registry = SseConnectionRegistry::new();
        let (_conn, _rx, _token) = registry.register("sub1", 1);

        let event1 = SseNotificationEvent {
            event_id: "n1".to_string(),
            data: "{}".to_string(),
        };
        registry.try_send("sub1", event1).unwrap();

        let event2 = SseNotificationEvent {
            event_id: "n2".to_string(),
            data: "{}".to_string(),
        };
        let result = registry.try_send("sub1", event2);
        assert!(matches!(result, Err(SseSendError::BufferFull { .. })));
    }

    #[tokio::test]
    async fn unregister_removes_connection() {
        let registry = SseConnectionRegistry::new();
        let (conn_id, _rx, _token) = registry.register("sub1", 8);
        assert!(registry.is_connected("sub1"));

        registry.unregister("sub1", conn_id);
        assert!(!registry.is_connected("sub1"));

        let event = SseNotificationEvent {
            event_id: "n1".to_string(),
            data: "{}".to_string(),
        };
        assert!(matches!(registry.try_send("sub1", event), Err(SseSendError::NoClient { .. })));
    }

    #[tokio::test]
    async fn is_connected_reflects_state() {
        let registry = SseConnectionRegistry::new();
        assert!(!registry.is_connected("sub1"));

        let (conn_id, _rx, _token) = registry.register("sub1", 8);
        assert!(registry.is_connected("sub1"));

        registry.unregister("sub1", conn_id);
        assert!(!registry.is_connected("sub1"));
    }

    #[tokio::test]
    async fn try_send_mixed_full_and_closed_returns_buffer_full() {
        let registry = SseConnectionRegistry::new();

        // Client A: buffer size 1, fill it so next send is Full.
        let (_conn_a, _rx_a, _token_a) = registry.register("sub1", 1);
        let fill = SseNotificationEvent {
            event_id: "fill".to_string(),
            data: "{}".to_string(),
        };
        registry.try_send("sub1", fill).unwrap();

        // Client B: drop the receiver so channel is Closed.
        let (_conn_b, rx_b, _token_b) = registry.register("sub1", 1);
        drop(rx_b);

        // Send again: A is Full, B is Closed. Should be BufferFull, not NoClient.
        let event = SseNotificationEvent {
            event_id: "n1".to_string(),
            data: "{}".to_string(),
        };
        let result = registry.try_send("sub1", event);
        assert!(matches!(result, Err(SseSendError::BufferFull { .. })));
    }
}
