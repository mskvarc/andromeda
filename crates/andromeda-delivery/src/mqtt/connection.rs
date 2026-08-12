//! One live broker connection and the state its event loop shares.

use crate::mqtt::{
    ack::AckTracker,
    connection_key::MqttConnectionKey,
    health::{ConnectionHealth, HealthCell},
    sys_topics::SysTopicData,
};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard, PoisonError as StdPoisonError};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::Instant,
};

/// The two client types `rumqttc` exposes, chosen by protocol version.
///
/// Enum-dispatched rather than boxed: there are exactly two and both are
/// known at connect time.
pub enum MqttClientVariant {
    /// An MQTT 5 client.
    V5 {
        /// The client publishes go out on.
        client: rumqttc::v5::AsyncClient,
    },
    /// An MQTT 3.1.1 client.
    V311 {
        /// The client publishes go out on.
        client: rumqttc::AsyncClient,
    },
}

impl MqttClientVariant {
    /// Ask the broker to close the session, ignoring a broker that has already
    /// gone.
    pub async fn disconnect(&self) {
        let sent = match self {
            MqttClientVariant::V5 { client } => client.disconnect().await.is_ok(),
            MqttClientVariant::V311 { client } => client.disconnect().await.is_ok(),
        };
        if !sent {
            tracing::debug!("MQTT disconnect not delivered; the connection was already gone");
        }
    }
}

/// When the next reconnection attempt to a failed broker is allowed.
pub struct ReconnectState {
    /// The backoff sequence, advanced once per failed attempt.
    pub backoff: backon::ExponentialBackoff,
    /// The earliest instant another attempt may be made.
    pub next_attempt_at: Instant,
}

/// State the event loop writes and the delivery and health paths read.
///
/// One `Arc` rather than three: the event loop needs all of it on every
/// packet, so splitting it into separate handles bought three refcount bumps
/// per connection and no isolation.
#[derive(Debug)]
pub struct ConnectionState {
    /// Current health, written by the event loop on every packet.
    pub health: HealthCell,
    /// When the connection was last known healthy.
    pub last_healthy: RwLock<Instant>,
    /// Broker metadata collected from `$SYS`.
    pub sys_data: RwLock<SysTopicData>,
    /// Correlates each outgoing QoS 1/2 publish with its broker acknowledgement.
    ///
    /// Written by the delivery path (register) and the event loop (assign,
    /// resolve, fail). A synchronous lock because every critical section is a
    /// few map operations with no `await` inside.
    pub ack_tracker: StdMutex<AckTracker>,
    /// Serialises the register-then-publish pair so a publish's registration
    /// order matches the order the event loop puts packets on the wire, which
    /// is what lets [`AckTracker`] assign packet ids to the right waiter.
    pub publish_lock: Mutex<()>,
}

impl ConnectionState {
    /// A state for a connection that has just come up.
    pub fn connected() -> ConnectionState {
        ConnectionState {
            health: HealthCell::new(ConnectionHealth::Healthy),
            last_healthy: RwLock::new(Instant::now()),
            sys_data: RwLock::new(SysTopicData::default()),
            ack_tracker: StdMutex::new(AckTracker::default()),
            publish_lock: Mutex::new(()),
        }
    }

    /// The acknowledgement tracker, recovering a poisoned lock rather than
    /// propagating a panic that the tracker's brief map operations cannot cause.
    pub fn acks(&self) -> StdMutexGuard<'_, AckTracker> {
        self.ack_tracker.lock().unwrap_or_else(StdPoisonError::into_inner)
    }

    /// Record that the broker is answering.
    ///
    /// The timestamp is written only if the lock is free: this runs on every
    /// inbound packet, and a health sweep holding the read lock is not a
    /// reason to stall the event loop. A missed write costs at most one probe
    /// interval of accuracy.
    pub fn mark_healthy(&self) {
        self.health.store(ConnectionHealth::Healthy);
        if let Ok(mut last) = self.last_healthy.try_write() {
            *last = Instant::now();
        }
    }

    /// Record that nothing has been heard for longer than expected.
    pub fn mark_degraded(&self) {
        self.health.store(ConnectionHealth::Degraded);
    }

    /// Record that the connection has failed.
    pub fn mark_unhealthy(&self) {
        self.health.store(ConnectionHealth::Unhealthy);
    }

    /// Whether the connection is answering.
    pub fn is_healthy(&self) -> bool {
        self.health.load() == ConnectionHealth::Healthy
    }
}

/// A pooled connection to one broker.
///
/// [`close`](MqttConnection::close) performs the graceful teardown — a broker
/// DISCONNECT, aborting the event loop, and failing every unacknowledged publish
/// — and the pool calls it wherever a connection is removed or replaced.
/// [`Drop`] aborts the event loop as a backstop, so a connection dropped without
/// `close` still cannot leave its spawned task polling an orphaned session.
pub struct MqttConnection {
    /// The client publishes go out on.
    pub client: MqttClientVariant,
    /// State shared with the event loop.
    pub state: Arc<ConnectionState>,
    /// The identity this connection is pooled under.
    pub key: MqttConnectionKey,
    /// The event loop driving the client, aborted by [`close`](MqttConnection::close)
    /// and by [`Drop`] so it never outlives the connection.
    pub event_loop: JoinHandle<()>,
}

impl MqttConnection {
    /// Tear the connection down: ask the broker to end the session, stop the
    /// event loop, and fail every publish still awaiting acknowledgement.
    ///
    /// The disconnect is best-effort — a broker that has already gone answers
    /// nothing — and the abort is what guarantees the spawned event loop does
    /// not outlive the connection. Failing the pending acknowledgements resolves
    /// their waiters to a retryable outcome so the delivery queue retransmits.
    pub async fn close(&self) {
        self.client.disconnect().await;
        self.event_loop.abort();
        self.state.acks().fail_all();
    }
}

impl Drop for MqttConnection {
    /// Stop the event loop if a connection is dropped without an explicit
    /// [`close`](MqttConnection::close). Aborting is synchronous and safe here;
    /// the graceful DISCONNECT and the ack failing that `close` performs are
    /// async and are not attempted from `Drop`.
    fn drop(&mut self) {
        self.event_loop.abort();
    }
}
