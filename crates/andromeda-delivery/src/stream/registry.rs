//! The live client connections one streaming protocol is holding open.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use dashmap::DashMap;
use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// Unique identifier for a single streaming client connection.
///
/// Allocated from a process-wide counter, so an identifier is unique across
/// every subscription and both protocols for the lifetime of the process.
/// That matters because a connection is unregistered by identifier and a
/// registry never sees the connections another registry handed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u64);

impl ConnectionId {
    /// Allocate the next unique connection identifier.
    pub fn next() -> ConnectionId {
        /// The process-wide allocator.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        ConnectionId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Which streaming protocol a registry serves.
///
/// Carried only so a send failure can name the protocol in its message. The
/// registry itself behaves identically for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum StreamKind {
    /// Server-Sent Events.
    #[strum(to_string = "SSE")]
    ServerSentEvents,
    /// WebSocket.
    #[strum(to_string = "WebSocket")]
    WebSocket,
}

/// A payload one streaming protocol delivers to a connected client.
///
/// Cloneable because a subscription may have several clients attached and each
/// gets its own copy of every event.
pub trait StreamPayload: Clone + fmt::Debug + Send + 'static {
    /// The protocol whose clients receive this payload.
    const KIND: StreamKind;
}

/// Errors from attempting to push to a subscription's connected clients.
#[derive(Debug, thiserror::Error)]
pub enum StreamSendError {
    /// No client is currently connected for the given subscription.
    #[error("no {kind} client connected for subscription {subscription_id}")]
    NoClient {
        /// The protocol that had nobody listening.
        kind: StreamKind,
        /// The subscription that was being delivered to.
        subscription_id: Identifier,
    },
    /// Every connected client's channel buffer is full.
    #[error("{kind} channel buffer full for all clients on subscription {subscription_id}")]
    BufferFull {
        /// The protocol whose clients are all behind.
        kind: StreamKind,
        /// The subscription that was being delivered to.
        subscription_id: Identifier,
    },
}

/// Thread-safe registry mapping subscription identifiers to live client
/// connections.
///
/// Each subscription supports several concurrent clients and every payload is
/// fanned out to all of them. Closed channels are reaped during a send rather
/// than by a sweeper: a client that has gone away is discovered by trying to
/// reach it, and there is no other moment at which the registry learns.
#[derive(Debug)]
pub struct ConnectionRegistry<P: StreamPayload> {
    /// Live connections, keyed by subscription and then by connection.
    connections: DashMap<Identifier, DashMap<ConnectionId, (mpsc::Sender<P>, CancellationToken)>>,
}

impl<P: StreamPayload> Default for ConnectionRegistry<P> {
    fn default() -> ConnectionRegistry<P> {
        ConnectionRegistry::new()
    }
}

impl<P: StreamPayload> ConnectionRegistry<P> {
    /// Create an empty connection registry.
    #[must_use]
    pub fn new() -> ConnectionRegistry<P> {
        ConnectionRegistry { connections: DashMap::new() }
    }

    /// Register a new client connection for a subscription.
    ///
    /// Returns the connection identifier, the receiving end of the payload
    /// channel, and a cancellation token that fires when the subscription is
    /// deleted.
    #[must_use]
    pub fn register(&self, subscription_id: &Identifier, buffer_size: usize) -> (ConnectionId, mpsc::Receiver<P>, CancellationToken) {
        let connection_id = ConnectionId::next();
        let (tx, rx) = mpsc::channel(buffer_size);
        let token = CancellationToken::new();
        self.connections
            .entry(subscription_id.clone()) // clone: the map owns the key and the caller keeps its borrow
            .or_default()
            .insert(connection_id, (tx, token.clone())); // clone: CancellationToken is Arc-based
        (connection_id, rx, token)
    }

    /// Remove one client connection, leaving the subscription's other clients
    /// untouched.
    pub fn unregister(&self, subscription_id: &Identifier, connection_id: ConnectionId) {
        if let Some(inner) = self.connections.get(subscription_id) {
            inner.remove(&connection_id);
        }
        self.connections.remove_if(subscription_id, |_, inner| inner.is_empty());
    }

    /// Push a payload to every client connected for a subscription.
    ///
    /// Succeeds when at least one client took it. The payload is borrowed and
    /// cloned once per client, so a subscription nobody is streaming clones
    /// nothing at all.
    ///
    /// # Errors
    ///
    /// Returns [`StreamSendError::BufferFull`] when every live client's channel
    /// is full, and [`StreamSendError::NoClient`] when the subscription has no
    /// connected client left — either none ever registered, or every one of
    /// them closed its channel. The distinction is what separates a slow
    /// consumer from an absent one: the caller retries the first and gives up
    /// on the second.
    pub fn try_send(&self, subscription_id: &Identifier, payload: &P) -> Result<(), StreamSendError> {
        let inner = self.connections.get(subscription_id).ok_or_else(|| StreamSendError::NoClient {
            kind: P::KIND,
            subscription_id: subscription_id.clone(), // clone: the error owns the subscription it names
        })?;

        let mut any_sent = false;
        let mut any_full = false;
        let mut closed_ids = Vec::new();

        for entry in inner.iter() {
            let (sender, _) = entry.value();
            match sender.try_send(payload.clone()) {
                // clone: each client gets its own copy
                Ok(()) => any_sent = true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    any_full = true;
                    debug!(
                        subscription_id = %subscription_id,
                        connection_id = ?entry.key(),
                        kind = %P::KIND,
                        "stream channel buffer full for client, skipping"
                    );
                }
                Err(mpsc::error::TrySendError::Closed(_)) => closed_ids.push(*entry.key()),
            }
        }

        for id in &closed_ids {
            inner.remove(id);
        }
        let remaining = inner.len();
        drop(inner);

        if !closed_ids.is_empty() && remaining == 0 {
            self.connections.remove_if(subscription_id, |_, clients| clients.is_empty());
        }

        if any_sent {
            Ok(())
        } else if any_full {
            Err(StreamSendError::BufferFull {
                kind: P::KIND,
                subscription_id: subscription_id.clone(), // clone: the error owns the subscription it names
            })
        } else {
            Err(StreamSendError::NoClient {
                kind: P::KIND,
                subscription_id: subscription_id.clone(), // clone: same
            })
        }
    }

    /// Close every connection for a subscription, signalling each stream to
    /// end.
    pub fn close(&self, subscription_id: &Identifier) {
        if let Some((_, inner)) = self.connections.remove(subscription_id) {
            for (_, (_, token)) in inner {
                token.cancel();
            }
        }
    }
}

/// Generate the registry conformance suite for one payload type.
///
/// Both streaming protocols use the same registry, so testing one and trusting
/// the other would leave the generic instantiation of the second unexercised.
/// The macro runs the identical scenarios against each, which is the only way
/// a divergence introduced by a payload's own `Clone` or `Send` bound would
/// show up.
#[cfg(test)]
macro_rules! registry_conformance_tests {
    ($module:ident, $payload:ty, $sample:expr, $kind:expr) => {
        mod $module {
            use crate::stream::registry::{ConnectionRegistry, StreamPayload, StreamSendError};
            use andromeda_ngsi_ld_lexicon::identifier::Identifier;
            use std::str::FromStr;

            fn sample() -> $payload {
                $sample
            }

            /// A well-formed subscription identifier from a short name.
            fn sub(name: &str) -> Identifier {
                Identifier::from_str(&format!("urn:ngsi-ld:Subscription:{name}")).expect("a valid subscription id")
            }

            #[tokio::test]
            async fn each_registration_gets_its_own_identifier() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();
                let (first, _rx_a, _token_a) = registry.register(&sub("sub1"), 8);
                let (second, _rx_b, _token_b) = registry.register(&sub("sub1"), 8);
                assert_ne!(first, second);
            }

            #[tokio::test]
            async fn a_payload_reaches_every_client_of_the_subscription() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();
                let (_, mut rx_a, _token_a) = registry.register(&sub("sub1"), 8);
                let (_, mut rx_b, _token_b) = registry.register(&sub("sub1"), 8);

                registry.try_send(&sub("sub1"), &sample()).expect("a client took it");

                pretty_assertions::assert_eq!(rx_a.recv().await.expect("a payload"), sample());
                pretty_assertions::assert_eq!(rx_b.recv().await.expect("a payload"), sample());
            }

            #[tokio::test]
            async fn unregistering_one_client_leaves_the_others_receiving() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();
                let (first, _rx_a, _token_a) = registry.register(&sub("sub1"), 8);
                let (_, mut rx_b, _token_b) = registry.register(&sub("sub1"), 8);

                registry.unregister(&sub("sub1"), first);

                registry.try_send(&sub("sub1"), &sample()).expect("the surviving client took it");
                pretty_assertions::assert_eq!(rx_b.recv().await.expect("a payload"), sample());
            }

            #[tokio::test]
            async fn closing_a_subscription_cancels_every_stream_and_ends_every_channel() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();
                let (_, mut rx_a, token_a) = registry.register(&sub("sub1"), 8);
                let (_, mut rx_b, token_b) = registry.register(&sub("sub1"), 8);

                registry.close(&sub("sub1"));

                assert!(token_a.is_cancelled());
                assert!(token_b.is_cancelled());
                assert!(rx_a.recv().await.is_none());
                assert!(rx_b.recv().await.is_none());
            }

            #[tokio::test]
            async fn a_subscription_nobody_is_streaming_reports_no_client() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();
                let error = registry.try_send(&sub("nonexistent"), &sample()).expect_err("nobody is listening");
                assert!(matches!(error, StreamSendError::NoClient { .. }));
                pretty_assertions::assert_eq!(
                    error.to_string(),
                    format!("no {} client connected for subscription {}", $kind, sub("nonexistent"))
                );
            }

            #[tokio::test]
            async fn a_subscription_whose_last_client_left_reports_no_client() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();
                let (only, _rx, _token) = registry.register(&sub("sub1"), 8);

                registry.unregister(&sub("sub1"), only);
                assert!(matches!(
                    registry.try_send(&sub("sub1"), &sample()),
                    Err(StreamSendError::NoClient { .. })
                ));
            }

            #[tokio::test]
            async fn a_client_that_cannot_keep_up_reports_a_full_buffer() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();
                let (_conn, _rx, _token) = registry.register(&sub("sub1"), 1);

                registry.try_send(&sub("sub1"), &sample()).expect("the first payload fits");
                assert!(matches!(
                    registry.try_send(&sub("sub1"), &sample()),
                    Err(StreamSendError::BufferFull { .. })
                ));
            }

            #[tokio::test]
            async fn one_full_client_and_one_gone_client_reports_full_rather_than_absent() {
                let registry: ConnectionRegistry<$payload> = ConnectionRegistry::new();

                let (_full, _rx_full, _token_full) = registry.register(&sub("sub1"), 1);
                registry.try_send(&sub("sub1"), &sample()).expect("the first payload fits");

                let (_gone, rx_gone, _token_gone) = registry.register(&sub("sub1"), 1);
                drop(rx_gone);

                assert!(matches!(
                    registry.try_send(&sub("sub1"), &sample()),
                    Err(StreamSendError::BufferFull { .. })
                ));
            }

            #[test]
            fn the_payload_declares_the_protocol_its_errors_name() {
                pretty_assertions::assert_eq!(<$payload as StreamPayload>::KIND.to_string(), $kind);
            }
        }
    };
}

#[cfg(test)]
pub(crate) use registry_conformance_tests;
