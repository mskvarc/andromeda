//! One live broker connection and the state its event loop shares.

use crate::mqtt::{
    connection_key::MqttConnectionKey,
    health::{ConnectionHealth, HealthCell},
    sys_topics::SysTopicData,
};
use std::sync::Arc;
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
}

impl ConnectionState {
    /// A state for a connection that has just come up.
    pub fn connected() -> ConnectionState {
        ConnectionState {
            health: HealthCell::new(ConnectionHealth::Healthy),
            last_healthy: RwLock::new(Instant::now()),
            sys_data: RwLock::new(SysTopicData::default()),
        }
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
/// Field order is the drop order. It does not by itself close the session
/// gracefully: dropping an `AsyncClient` sends no DISCONNECT and dropping a
/// `JoinHandle` detaches its task unconditionally, so a graceful shutdown is
/// performed explicitly through `disconnect()` before this value is dropped.
pub struct MqttConnection {
    /// The client publishes go out on.
    pub client: MqttClientVariant,
    /// State shared with the event loop.
    pub state: Arc<ConnectionState>,
    /// When another connection attempt is allowed, once this one has failed.
    pub reconnect_state: Mutex<ReconnectState>,
    /// The identity this connection is pooled under.
    pub key: MqttConnectionKey,
    /// The event loop driving the client. Held so it lives as long as the
    /// connection does.
    pub event_loop: JoinHandle<()>,
}

impl MqttConnection {
    /// Whether another connection attempt to this broker is allowed yet.
    pub async fn should_reconnect_now(&self) -> bool {
        let state = self.reconnect_state.lock().await;
        Instant::now() >= state.next_attempt_at
    }
}
