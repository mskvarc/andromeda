//! The broker connections this process is holding open.

use crate::{
    endpoint::mqtt::MqttEndpointConfig,
    error::TransportError,
    mqtt::{
        config::MqttPoolConfig,
        connection::{ConnectionState, MqttConnection, ReconnectState},
        connection_key::MqttConnectionKey,
        dial::MqttDialer,
        handle::MqttTransportHandle,
        health::MqttConnectionHealthSummary,
    },
};
use backon::BackoffBuilder;
use dashmap::DashMap;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::Instant,
};
use tracing::{debug, info};

/// The delay used when the backoff sequence has nothing left to give.
const BACKOFF_EXHAUSTED_DELAY: Duration = Duration::from_secs(60);

/// How many keep-alive intervals of silence make a connection degraded.
const DEGRADED_AFTER_KEEPALIVES: u64 = 2;

/// Pooled MQTT connection manager with lifecycle management.
pub struct MqttTransportPool {
    /// Live connections, keyed by broker identity.
    connections: DashMap<MqttConnectionKey, Arc<MqttConnection>>,
    /// Reconnect backoff per broker identity, held independently of whether a
    /// live connection exists so a failed first dial still arms the backoff and
    /// a broker that is refusing connections is not dialled on every delivery.
    reconnect: DashMap<MqttConnectionKey, ReconnectState>,
    /// One dial guard per broker identity, so concurrent first-misses to the
    /// same broker cannot each open a connection and silently detach the loser's
    /// event-loop task.
    dial_locks: DashMap<MqttConnectionKey, Arc<Mutex<()>>>,
    /// Limits and timings.
    config: MqttPoolConfig,
    /// Capability that opens a broker session.
    dialer: Arc<dyn MqttDialer>,
}

impl MqttTransportPool {
    /// An empty pool.
    pub fn new(config: MqttPoolConfig, dialer: Arc<dyn MqttDialer>) -> MqttTransportPool {
        MqttTransportPool {
            connections: DashMap::new(),
            reconnect: DashMap::new(),
            dial_locks: DashMap::new(),
            config,
            dialer,
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
                        let connections: Vec<_> = pool.connections.iter().map(|entry| (entry.key().clone(), Arc::clone(entry.value()))).collect();
                        for (key, connection) in connections {
                            let last = *connection.state.last_healthy.read().await;
                            if last.elapsed().as_secs() > quiet_threshold && connection.state.is_healthy() {
                                connection.state.mark_degraded();
                                debug!(
                                    host = %key.authority.host(),
                                    port = key.authority.port(),
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

        // A live, answering connection needs no dial.
        if let Some(entry) = self.connections.get(&key)
            && entry.value().state.is_healthy()
        {
            return Ok(Arc::clone(entry.value()));
        }

        // Serialise dials to one broker: two concurrent first-misses must not
        // each open a connection and detach the loser's event loop.
        let dial_lock = self.dial_lock_for(&key);
        let _dialling = dial_lock.lock().await;

        // Another dial may have won the race while this one waited for the lock.
        if let Some(entry) = self.connections.get(&key)
            && entry.value().state.is_healthy()
        {
            return Ok(Arc::clone(entry.value()));
        }

        // A broker still inside its reconnect backoff is not dialled; the
        // delivery fails without touching it. The backoff is keyed by broker
        // identity, not by a live connection, so it holds even after the failed
        // connection was removed and on a broker that was never reached at all.
        if !self.may_dial(&key) {
            return Err(TransportError::ConnectionFailed {
                endpoint: config.endpoint_uri.clone(), // clone: the error owns the endpoint it names
                detail: "connection unhealthy, backoff active".to_string(),
            });
        }

        // Tear down any stale connection under this key before replacing it, so
        // its event loop stops and its unacknowledged publishes fail retryably.
        if let Some((_, stale)) = self.connections.remove(&key) {
            stale.close().await;
        }

        if self.connections.len() >= self.config.max_connections {
            self.evict_stalest().await;
        }

        match self.create_connection(&key, config).await {
            Ok(connection) => {
                self.clear_backoff(&key);
                let connection = Arc::new(connection);
                self.connections.insert(key, Arc::clone(&connection));
                Ok(connection)
            }
            Err(error) => {
                self.advance_backoff(&key);
                Err(error)
            }
        }
    }

    /// The dial guard for a broker, created on first need.
    fn dial_lock_for(&self, key: &MqttConnectionKey) -> Arc<Mutex<()>> {
        // clone: the guard outlives the map entry, and the key is owned by the map
        Arc::clone(self.dial_locks.entry(key.clone()).or_default().value())
    }

    /// Whether a broker's reconnect backoff has elapsed. A broker with no
    /// recorded backoff has never failed, or was cleared on a successful dial,
    /// and may be dialled at once.
    fn may_dial(&self, key: &MqttConnectionKey) -> bool {
        self.reconnect.get(key).is_none_or(|state| Instant::now() >= state.next_attempt_at)
    }

    /// Advance the reconnect backoff for a broker whose dial just failed, arming
    /// the entry on the first failure.
    fn advance_backoff(&self, key: &MqttConnectionKey) {
        // clone: the map owns the key; the backoff outlives this call
        let mut state = self.reconnect.entry(key.clone()).or_insert_with(|| ReconnectState {
            backoff: self.config.reconnect_backoff.build(),
            next_attempt_at: Instant::now(),
        });
        let delay = state.backoff.next().unwrap_or(BACKOFF_EXHAUSTED_DELAY);
        state.next_attempt_at = Instant::now() + delay;
    }

    /// Forget a broker's backoff once a dial to it has succeeded.
    fn clear_backoff(&self, key: &MqttConnectionKey) {
        self.reconnect.remove(key);
    }

    /// Drop the connection that has been idle longest, making room in the pool.
    async fn evict_stalest(&self) {
        let mut stalest_key: Option<MqttConnectionKey> = None;
        let mut stalest_time = Instant::now();

        let connections: Vec<_> = self.connections.iter().map(|entry| (entry.key().clone(), Arc::clone(entry.value()))).collect();
        for (key, connection) in connections {
            let last = *connection.state.last_healthy.read().await;
            if last < stalest_time {
                stalest_time = last;
                stalest_key = Some(key);
            }
        }

        if let Some(key) = stalest_key
            && let Some((_, connection)) = self.connections.remove(&key)
        {
            info!(
                host = %key.authority.host(),
                port = key.authority.port(),
                "evicted stalest MQTT connection to make room in pool"
            );
            connection.close().await;
        }
    }

    /// Open a connection to the broker the key names.
    async fn create_connection(&self, key: &MqttConnectionKey, config: &MqttEndpointConfig) -> Result<MqttConnection, TransportError> {
        let state = Arc::new(ConnectionState::connected());
        let (client, event_loop) = self.dialer.dial(key, config, &self.config, &state).await?;

        Ok(MqttConnection {
            client,
            state,
            key: key.clone(), // clone: the connection outlives the lookup that created it
            event_loop,
        })
    }

    /// Health summaries for every pooled connection.
    pub async fn connection_health_summaries(&self) -> Vec<MqttConnectionHealthSummary> {
        let connections: Vec<_> = self.connections.iter().map(|entry| Arc::clone(entry.value())).collect();
        let mut summaries = Vec::with_capacity(connections.len());
        for connection in connections {
            let last = *connection.state.last_healthy.read().await;
            let sys = connection.state.sys_data.read().await;
            summaries.push(MqttConnectionHealthSummary {
                authority: connection.key.authority.clone(), // clone: the summary outlives the pool entry it was read from
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
        let connections: Vec<_> = self.connections.iter().map(|entry| Arc::clone(entry.value())).collect();
        for connection in connections {
            connection.close().await;
        }
        self.connections.clear();
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint::{
            mqtt::{MqttEndpointConfig, MqttQoS, MqttVersion},
            topic::MqttTopic,
        },
        host_port::HostPort,
        mqtt::{
            ack::AckKind,
            config::MqttPoolConfig,
            connection::{ConnectionState, MqttClientVariant, MqttConnection},
            connection_key::MqttConnectionKey,
            dial::{DialFuture, MqttDialer, RumqttcDialer},
            pool::MqttTransportPool,
        },
    };
    use backon::ExponentialBuilder;
    use std::{
        str::FromStr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use url::Url;

    /// An endpoint whose broker refuses connections: port 1 on the loopback is
    /// never listening, so a dial fails fast rather than timing out.
    fn refused_endpoint() -> MqttEndpointConfig {
        MqttEndpointConfig {
            authority: HostPort::from_str("127.0.0.1:1").expect("a valid authority"),
            topic: MqttTopic::from_str("andromeda/test").expect("a valid topic"),
            endpoint_uri: Url::parse("mqtt://127.0.0.1:1/andromeda/test").expect("a valid endpoint URI"),
            use_tls: false,
            version: MqttVersion::V311,
            qos: MqttQoS::AtMost,
            username: None,
            password: None,
        }
    }

    /// A key for a hand-built connection that never dials.
    fn test_key() -> MqttConnectionKey {
        MqttConnectionKey {
            authority: HostPort::from_str("127.0.0.1:1883").expect("a valid authority"),
            use_tls: false,
            version: MqttVersion::V311,
            username: None,
            password_hash: None,
        }
    }

    /// A deterministic dial capability that creates inert clients without a socket.
    struct ScriptedDialer {
        /// Number of calls made through the pool.
        calls: AtomicUsize,
    }

    impl ScriptedDialer {
        /// A dialer whose every scripted dial succeeds.
        const fn succeeding() -> ScriptedDialer {
            ScriptedDialer { calls: AtomicUsize::new(0) }
        }
    }

    impl MqttDialer for ScriptedDialer {
        fn dial<'a>(
            &'a self,
            _key: &'a MqttConnectionKey,
            _endpoint: &'a MqttEndpointConfig,
            _pool: &'a MqttPoolConfig,
            _state: &'a Arc<ConnectionState>,
        ) -> DialFuture<'a> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {
                let (client, _eventloop) = rumqttc::AsyncClient::new(rumqttc::MqttOptions::new("scripted", "127.0.0.1", 1883), 8);
                Ok((MqttClientVariant::V311 { client }, tokio::spawn(std::future::pending())))
            })
        }
    }

    #[tokio::test]
    async fn a_pool_that_has_opened_nothing_reports_no_connections() {
        let pool = MqttTransportPool::new(MqttPoolConfig::default(), Arc::new(RumqttcDialer));
        assert!(pool.connection_health_summaries().await.is_empty());
    }

    #[tokio::test]
    async fn a_scripted_dial_is_reused_for_the_same_connection_key() {
        let dialer = Arc::new(ScriptedDialer::succeeding());
        let pool = MqttTransportPool::new(MqttPoolConfig::default(), Arc::clone(&dialer) as Arc<dyn MqttDialer>);
        let endpoint = refused_endpoint();

        let first = pool.get_or_create_connection(&endpoint).await.expect("scripted dial succeeds");
        let second = pool.get_or_create_connection(&endpoint).await.expect("healthy connection is reused");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(dialer.calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_pool_that_has_opened_nothing_is_healthy() {
        let pool = MqttTransportPool::new(MqttPoolConfig::default(), Arc::new(RumqttcDialer));
        assert!(pool.all_healthy());
    }

    #[tokio::test]
    async fn a_failed_dial_arms_the_backoff_so_the_next_attempt_is_refused_until_it_lifts() {
        // A short, jitter-free backoff so the test's timing is deterministic.
        let config = MqttPoolConfig {
            connect_timeout: Duration::from_secs(2),
            reconnect_backoff: ExponentialBuilder::default()
                .with_min_delay(Duration::from_millis(200))
                .with_max_delay(Duration::from_millis(400))
                .without_max_times(),
            ..MqttPoolConfig::default()
        };
        let pool = MqttTransportPool::new(config, Arc::new(RumqttcDialer));
        let endpoint = refused_endpoint();

        // First dial: the broker refuses, so the connection fails and the backoff
        // arms — even though no connection was ever inserted under the key.
        assert!(pool.get_or_create_connection(&endpoint).await.is_err());

        // Second dial, immediately: inside the backoff window, refused without a
        // dial. This is the case the pre-fix code let through, because the backoff
        // state lived on a connection that never existed.
        let refused = pool.get_or_create_connection(&endpoint).await.map(drop).expect_err("the backoff refuses");
        assert!(refused.to_string().contains("backoff active"), "{refused}");

        // Once the backoff elapses, a dial is permitted again (and fails on its
        // own terms — the broker is still not there).
        tokio::time::sleep(Duration::from_millis(500)).await;
        let permitted = pool.get_or_create_connection(&endpoint).await.map(drop).expect_err("the broker is still down");
        assert!(!permitted.to_string().contains("backoff active"), "{permitted}");
    }

    #[tokio::test]
    async fn closing_a_connection_fails_pending_publishes_and_aborts_the_event_loop() {
        // A client with no live session; its event loop is never polled here.
        let (client, _eventloop) = rumqttc::AsyncClient::new(rumqttc::MqttOptions::new("test", "127.0.0.1", 1883), 8);
        let state = Arc::new(ConnectionState::connected());
        // A publish waiting on its PUBACK.
        let mut ack = state.acks().register(AckKind::PubAck);
        // A stand-in event loop that never ends on its own, so only the abort can stop it.
        let event_loop = tokio::spawn(std::future::pending::<()>());
        let connection = MqttConnection {
            client: MqttClientVariant::V311 { client },
            state: Arc::clone(&state),
            key: test_key(),
            event_loop,
        };

        connection.close().await;

        // `fail_all` resolved the waiter to a cancellation the delivery path retries.
        assert!(ack.try_recv().is_err(), "the pending publish is failed when the connection closes");

        // The spawned event loop was aborted rather than left polling an orphaned session.
        for _ in 0..20 {
            if connection.event_loop.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(connection.event_loop.is_finished(), "the event loop task is aborted");
    }
}
