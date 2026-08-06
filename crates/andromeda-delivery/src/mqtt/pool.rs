//! The broker connections this process is holding open.

use crate::{
    endpoint::mqtt::MqttEndpointConfig,
    error::TransportError,
    mqtt::{
        config::MqttPoolConfig,
        connection::{ConnectionState, MqttConnection, ReconnectState},
        connection_key::MqttConnectionKey,
        dial::dial,
        handle::MqttTransportHandle,
        health::MqttConnectionHealthSummary,
    },
};
use backon::BackoffBuilder;
use dashmap::DashMap;
use std::{sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle, time::Instant};
use tracing::{debug, info};

/// The delay used when the backoff sequence has nothing left to give.
const BACKOFF_EXHAUSTED_DELAY: Duration = Duration::from_secs(60);

/// How many keep-alive intervals of silence make a connection degraded.
const DEGRADED_AFTER_KEEPALIVES: u64 = 2;

/// Pooled MQTT connection manager with lifecycle management.
pub struct MqttTransportPool {
    /// Live connections, keyed by broker identity.
    connections: DashMap<MqttConnectionKey, Arc<MqttConnection>>,
    /// Limits and timings.
    config: MqttPoolConfig,
}

impl MqttTransportPool {
    /// An empty pool.
    pub fn new(config: MqttPoolConfig) -> MqttTransportPool {
        MqttTransportPool {
            connections: DashMap::new(),
            config,
        }
    }

    /// A transport handle bound to one MQTT endpoint.
    pub fn transport_for(self: &Arc<MqttTransportPool>, config: &MqttEndpointConfig) -> MqttTransportHandle {
        MqttTransportHandle::new(Arc::clone(self), config.clone()) // clone: the handle outlives the subscription it was read from
    }

    /// How long a QoS 1/2 publish waits for its broker acknowledgement.
    pub const fn publish_ack_timeout(&self) -> Duration {
        self.config.publish_ack_timeout
    }

    /// Spawn a background task that marks connections degraded once they have
    /// gone quiet for longer than the protocol allows.
    ///
    /// This is not a probe in the sense of sending anything: MQTT's own
    /// keep-alive already does that, and a broker that has stopped answering it
    /// will stop producing packets. What this notices is the absence.
    pub fn spawn_health_probe(self: &Arc<MqttTransportPool>, mut shutdown_rx: watch::Receiver<bool>) -> JoinHandle<()> {
        let pool = Arc::clone(self);
        let interval = pool.config.health_probe_interval;
        let quiet_threshold = pool.config.keepalive.as_secs() * DEGRADED_AFTER_KEEPALIVES;

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = tick.tick() => {
                        for entry in pool.connections.iter() {
                            let connection = entry.value();
                            let last = *connection.state.last_healthy.read().await;
                            if last.elapsed().as_secs() > quiet_threshold && connection.state.is_healthy() {
                                connection.state.mark_degraded();
                                debug!(
                                    host = %entry.key().host,
                                    port = entry.key().port,
                                    "MQTT connection marked degraded (no activity beyond 2x keepalive)"
                                );
                            }
                        }
                    }
                }
            }
        })
    }

    /// The connection for an endpoint, opening one if the pool has none that
    /// is answering.
    pub async fn get_or_create_connection(&self, config: &MqttEndpointConfig) -> Result<Arc<MqttConnection>, TransportError> {
        let key = MqttConnectionKey::from_config(config);

        if let Some(entry) = self.connections.get(&key)
            && entry.value().state.is_healthy()
        {
            return Ok(Arc::clone(entry.value()));
        }

        // A connection that is present but not answering is either past its
        // backoff, in which case it is replaced, or inside it, in which case
        // the delivery fails without touching the broker.
        if let Some(entry) = self.connections.get(&key) {
            let connection = entry.value();
            if !connection.state.is_healthy() {
                if !connection.should_reconnect_now().await {
                    return Err(TransportError::ConnectionFailed {
                        endpoint: format!("{}:{}", config.host, config.port),
                        detail: "connection unhealthy, backoff active".to_string(),
                    });
                }
                drop(entry);
                self.connections.remove(&key);
            }
        }

        if self.connections.len() >= self.config.max_connections {
            self.evict_stalest().await;
        }

        match self.create_connection(&key, config).await {
            Ok(connection) => {
                let connection = Arc::new(connection);
                self.connections.insert(key, Arc::clone(&connection));
                Ok(connection)
            }
            Err(error) => {
                // Advance the backoff on whatever placeholder is there, so a
                // broker that is refusing connections is not hammered.
                if let Some(entry) = self.connections.get(&key) {
                    let mut state = entry.value().reconnect_state.lock().await;
                    let delay = state.backoff.next().unwrap_or(BACKOFF_EXHAUSTED_DELAY);
                    state.next_attempt_at = Instant::now() + delay;
                }
                Err(error)
            }
        }
    }

    /// Close the connection an endpoint was using, if the pool still has it.
    pub async fn disconnect(&self, key: &MqttConnectionKey) {
        if let Some((_, connection)) = self.connections.remove(key) {
            connection.client.disconnect().await;
        }
    }

    /// Drop the connection that has been idle longest, making room in the pool.
    async fn evict_stalest(&self) {
        let mut stalest_key: Option<MqttConnectionKey> = None;
        let mut stalest_time = Instant::now();

        for entry in self.connections.iter() {
            let last = *entry.value().state.last_healthy.read().await;
            if last < stalest_time {
                stalest_time = last;
                stalest_key = Some(entry.key().clone()); // clone: the key outlives the iteration that found it
            }
        }

        if let Some(key) = stalest_key
            && let Some((_, connection)) = self.connections.remove(&key)
        {
            info!(
                host = %key.host,
                port = key.port,
                "evicted stalest MQTT connection to make room in pool"
            );
            connection.client.disconnect().await;
        }
    }

    /// Open a connection to the broker the key names.
    async fn create_connection(&self, key: &MqttConnectionKey, config: &MqttEndpointConfig) -> Result<MqttConnection, TransportError> {
        let state = Arc::new(ConnectionState::connected());
        let (client, event_loop) = dial(key, config, &self.config, &state).await?;

        Ok(MqttConnection {
            client,
            state,
            reconnect_state: tokio::sync::Mutex::new(ReconnectState {
                backoff: self.config.reconnect_backoff.build(),
                next_attempt_at: Instant::now(),
            }),
            key: key.clone(), // clone: the connection outlives the lookup that created it
            event_loop,
        })
    }

    /// Health summaries for every pooled connection.
    pub async fn connection_health_summaries(&self) -> Vec<MqttConnectionHealthSummary> {
        let mut summaries = Vec::with_capacity(self.connections.len());
        for entry in self.connections.iter() {
            let connection = entry.value();
            let last = *connection.state.last_healthy.read().await;
            let sys = connection.state.sys_data.read().await;
            summaries.push(MqttConnectionHealthSummary {
                host: connection.key.host.clone(), // clone: the summary outlives the pool entry it was read from
                port: connection.key.port,
                version: connection.key.version,
                health: connection.state.health.load(),
                seconds_since_healthy: last.elapsed().as_secs(),
                broker_uptime: sys.broker_uptime.clone(),         // clone: same
                broker_version: sys.broker_version.clone(),       // clone: same
                clients_connected: sys.clients_connected.clone(), // clone: same
            });
        }
        summaries
    }

    /// Whether every pooled connection is answering, which an empty pool
    /// vacuously is.
    pub fn all_healthy(&self) -> bool {
        self.connections.iter().all(|entry| entry.value().state.is_healthy())
    }

    /// Disconnect every pooled connection.
    pub async fn shutdown_all(&self) {
        for entry in self.connections.iter() {
            entry.value().client.disconnect().await;
        }
        self.connections.clear();
    }
}

#[cfg(test)]
mod tests {
    use crate::mqtt::{config::MqttPoolConfig, pool::MqttTransportPool};

    #[tokio::test]
    async fn a_pool_that_has_opened_nothing_reports_no_connections() {
        let pool = MqttTransportPool::new(MqttPoolConfig::default());
        assert!(pool.connection_health_summaries().await.is_empty());
    }

    #[test]
    fn a_pool_that_has_opened_nothing_is_healthy() {
        let pool = MqttTransportPool::new(MqttPoolConfig::default());
        assert!(pool.all_healthy());
    }
}
