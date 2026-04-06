//! MQTT notification transport with connection pooling.
//!
//! Implements [`NotificationTransport`] for MQTT brokers via [`MqttTransportPool`]
//! with connection lifecycle management, health probing, reconnection backoff,
//! and connection limits.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use backon::BackoffBuilder;
use dashmap::DashMap;
use serde_json::Value;
use tokio::{sync::RwLock, time::Instant};
use tracing::{debug, error, info, warn};

use crate::{
    http::header::link::Link,
    transport::{
        endpoint::{MqttEndpointConfig, MqttQoS, MqttVersion},
        types::{DeliveryOutcome, DeliveryRequest, NotificationTransport, TransportError},
    },
};

// Health state constants.
const HEALTH_HEALTHY: u8 = 0;
const HEALTH_DEGRADED: u8 = 1;
const HEALTH_UNHEALTHY: u8 = 2;

/// Connection cache key including credentials hash.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct MqttConnectionKey {
    host: String,
    port: u16,
    use_tls: bool,
    version: MqttVersion,
    username: Option<String>,
    password_hash: Option<u64>,
}

impl MqttConnectionKey {
    fn from_config(config: &MqttEndpointConfig) -> MqttConnectionKey {
        let password_hash = config.password.as_ref().map(|p| {
            let mut hasher = DefaultHasher::new();
            p.hash(&mut hasher);
            hasher.finish()
        });
        MqttConnectionKey {
            host: config.host.clone(),
            port: config.port,
            use_tls: config.use_tls,
            version: config.version,
            username: config.username.clone(),
            password_hash,
        }
    }
}

/// MQTT connection pool configuration.
#[derive(Debug, Clone)]
pub struct MqttPoolConfig {
    pub max_connections: usize,
    pub connect_timeout: Duration,
    pub keepalive: Duration,
    pub reconnect_backoff: backon::ExponentialBuilder,
    pub health_probe_interval: Duration,
}

impl Default for MqttPoolConfig {
    fn default() -> MqttPoolConfig {
        MqttPoolConfig {
            max_connections: 100,
            connect_timeout: Duration::from_secs(5),
            keepalive: Duration::from_secs(30),
            reconnect_backoff: backon::ExponentialBuilder::default()
                .with_min_delay(Duration::from_secs(1))
                .with_max_delay(Duration::from_secs(60))
                .with_factor(2.0)
                .with_jitter()
                .without_max_times(),
            health_probe_interval: Duration::from_secs(30),
        }
    }
}

struct ReconnectState {
    backoff_iter: backon::ExponentialBackoff,
    next_attempt_at: Instant,
}

enum MqttClientVariant {
    V5 { client: rumqttc::v5::AsyncClient },
    V311 { client: rumqttc::AsyncClient },
}

struct MqttConnection {
    client: MqttClientVariant,
    health: Arc<AtomicU8>,
    last_healthy: Arc<RwLock<Instant>>,
    reconnect_state: tokio::sync::Mutex<ReconnectState>,
    _event_loop: tokio::task::JoinHandle<()>,
}

impl MqttConnection {
    fn is_healthy(&self) -> bool {
        self.health.load(Ordering::Acquire) == HEALTH_HEALTHY
    }

    fn mark_healthy(&self) {
        self.health.store(HEALTH_HEALTHY, Ordering::Release);
        // Update last_healthy in a non-blocking way; if the lock is held, skip.
        if let Ok(mut last) = self.last_healthy.try_write() {
            *last = Instant::now();
        }
    }

    fn mark_degraded(&self) {
        self.health.store(HEALTH_DEGRADED, Ordering::Release);
    }

    fn mark_unhealthy(&self) {
        self.health.store(HEALTH_UNHEALTHY, Ordering::Release);
    }

    async fn should_reconnect_now(&self) -> bool {
        let state = self.reconnect_state.lock().await;
        Instant::now() >= state.next_attempt_at
    }
}

/// Pooled MQTT connection manager with lifecycle management.
pub struct MqttTransportPool {
    connections: DashMap<MqttConnectionKey, Arc<MqttConnection>>,
    config: MqttPoolConfig,
}

impl MqttTransportPool {
    pub fn new(config: MqttPoolConfig) -> MqttTransportPool {
        MqttTransportPool {
            connections: DashMap::new(),
            config,
        }
    }

    /// Return a transport handle bound to a specific MQTT endpoint.
    pub fn transport_for(self: &Arc<MqttTransportPool>, config: &MqttEndpointConfig) -> MqttTransportHandle {
        MqttTransportHandle {
            pool: Arc::clone(self),
            config: config.clone(),
        }
    }

    /// Spawn a background task that periodically checks connection health.
    pub fn spawn_health_probe(self: &Arc<MqttTransportPool>, mut shutdown_rx: tokio::sync::watch::Receiver<bool>) -> tokio::task::JoinHandle<()> {
        let pool = Arc::clone(self);
        let interval = pool.config.health_probe_interval;
        let keepalive_threshold = pool.config.keepalive.as_secs() * 2;

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = tick.tick() => {
                        for entry in pool.connections.iter() {
                            let conn = entry.value();
                            let last = *conn.last_healthy.read().await;
                            if last.elapsed().as_secs() > keepalive_threshold && conn.is_healthy() {
                                conn.mark_degraded();
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

    async fn get_or_create_connection(&self, config: &MqttEndpointConfig) -> Result<Arc<MqttConnection>, TransportError> {
        let key = MqttConnectionKey::from_config(config);

        // Fast path: existing healthy connection.
        if let Some(entry) = self.connections.get(&key)
            && entry.value().is_healthy()
        {
            return Ok(Arc::clone(entry.value()));
        }

        // Check if an unhealthy connection exists and whether we should reconnect.
        if let Some(entry) = self.connections.get(&key) {
            let conn = entry.value();
            if !conn.is_healthy() {
                if !conn.should_reconnect_now().await {
                    return Err(TransportError::ConnectionFailed {
                        endpoint: format!("{}:{}", config.host, config.port),
                        detail: "connection unhealthy, backoff active".to_string(),
                    });
                }
                // Drop the ref before removing.
                drop(entry);
                self.connections.remove(&key);
            }
        }

        // Enforce pool limit.
        if self.connections.len() >= self.config.max_connections {
            self.evict_stalest().await;
        }

        // Create a new connection, updating backoff state on failure.
        match self.create_connection(&key, config).await {
            Ok(conn) => {
                let conn = Arc::new(conn);
                self.connections.insert(key, Arc::clone(&conn));
                Ok(conn)
            }
            Err(e) => {
                // Insert a placeholder unhealthy connection so future attempts
                // respect backoff instead of hammering the broker.
                if let Some(entry) = self.connections.get(&key) {
                    let mut state = entry.value().reconnect_state.lock().await;
                    let delay = state.backoff_iter.next().unwrap_or(Duration::from_secs(60));
                    state.next_attempt_at = Instant::now() + delay;
                }
                Err(e)
            }
        }
    }

    async fn evict_stalest(&self) {
        let mut stalest_key: Option<MqttConnectionKey> = None;
        let mut stalest_time = Instant::now();

        for entry in self.connections.iter() {
            let last = *entry.value().last_healthy.read().await;
            if last < stalest_time {
                stalest_time = last;
                stalest_key = Some(entry.key().clone());
            }
        }

        if let Some(key) = stalest_key
            && let Some((_, conn)) = self.connections.remove(&key)
        {
            info!(
                host = %key.host,
                port = key.port,
                "evicted stalest MQTT connection to make room in pool"
            );
            match &conn.client {
                MqttClientVariant::V5 { client } => {
                    let _ = client.disconnect().await;
                }
                MqttClientVariant::V311 { client } => {
                    let _ = client.disconnect().await;
                }
            }
        }
    }

    async fn create_connection(&self, key: &MqttConnectionKey, config: &MqttEndpointConfig) -> Result<MqttConnection, TransportError> {
        let client_id = format!("andromeda-{}", uuid::Uuid::new_v4());
        let healthy = Arc::new(AtomicU8::new(HEALTH_HEALTHY));
        let last_healthy = Arc::new(RwLock::new(Instant::now()));
        let endpoint_str = format!("{}:{}", key.host, key.port);

        let version_label = config.version.to_string();

        match key.version {
            MqttVersion::V5 => {
                let mut options = rumqttc::v5::MqttOptions::new(&client_id, &key.host, key.port);
                options.set_keep_alive(self.config.keepalive);
                if key.use_tls {
                    options.set_transport(rumqttc::Transport::tls_with_default_config());
                }
                if let Some(ref username) = config.username {
                    options.set_credentials(username.clone(), config.password.clone().unwrap_or_default());
                }

                let (client, mut eventloop) = rumqttc::v5::AsyncClient::new(options, 64);

                match tokio::time::timeout(self.config.connect_timeout, eventloop.poll()).await {
                    Ok(Ok(_)) => {
                        info!(version = %version_label, host = %key.host, port = key.port, "MQTT connected");
                    }
                    Ok(Err(e)) => {
                        return Err(TransportError::ConnectionFailed {
                            endpoint: endpoint_str,
                            detail: format!("MQTT {version_label} connection failed: {e}"),
                        });
                    }
                    Err(_) => {
                        return Err(TransportError::Timeout {
                            endpoint: endpoint_str,
                            duration: self.config.connect_timeout,
                        });
                    }
                }

                let loop_healthy = healthy.clone();
                let loop_last_healthy = last_healthy.clone();
                let host = key.host.clone();
                let port = key.port;
                let vl = version_label.clone();
                let task = tokio::spawn(async move {
                    loop {
                        match eventloop.poll().await {
                            Ok(_) => {
                                loop_healthy.store(HEALTH_HEALTHY, Ordering::Release);
                                if let Ok(mut last) = loop_last_healthy.try_write() {
                                    *last = Instant::now();
                                }
                            }
                            Err(e) => {
                                warn!(
                                    version = %vl,
                                    host = %host,
                                    port,
                                    error = %e,
                                    "MQTT event loop error — marking unhealthy"
                                );
                                loop_healthy.store(HEALTH_UNHEALTHY, Ordering::Release);
                                return;
                            }
                        }
                    }
                });

                Ok(MqttConnection {
                    client: MqttClientVariant::V5 { client },
                    health: healthy,
                    last_healthy,
                    reconnect_state: tokio::sync::Mutex::new(ReconnectState {
                        backoff_iter: self.config.reconnect_backoff.build(),
                        next_attempt_at: Instant::now(),
                    }),
                    _event_loop: task,
                })
            }
            MqttVersion::V311 => {
                let mut options = rumqttc::MqttOptions::new(&client_id, &key.host, key.port);
                options.set_keep_alive(self.config.keepalive);
                if key.use_tls {
                    options.set_transport(rumqttc::Transport::tls_with_default_config());
                }
                if let Some(ref username) = config.username {
                    options.set_credentials(username.clone(), config.password.clone().unwrap_or_default());
                }

                let (client, mut eventloop) = rumqttc::AsyncClient::new(options, 64);

                match tokio::time::timeout(self.config.connect_timeout, eventloop.poll()).await {
                    Ok(Ok(_)) => {
                        info!(version = %version_label, host = %key.host, port = key.port, "MQTT connected");
                    }
                    Ok(Err(e)) => {
                        return Err(TransportError::ConnectionFailed {
                            endpoint: endpoint_str,
                            detail: format!("MQTT {version_label} connection failed: {e}"),
                        });
                    }
                    Err(_) => {
                        return Err(TransportError::Timeout {
                            endpoint: endpoint_str,
                            duration: self.config.connect_timeout,
                        });
                    }
                }

                let loop_healthy = healthy.clone();
                let loop_last_healthy = last_healthy.clone();
                let host = key.host.clone();
                let port = key.port;
                let vl = version_label.clone();
                let task = tokio::spawn(async move {
                    loop {
                        match eventloop.poll().await {
                            Ok(_) => {
                                loop_healthy.store(HEALTH_HEALTHY, Ordering::Release);
                                if let Ok(mut last) = loop_last_healthy.try_write() {
                                    *last = Instant::now();
                                }
                            }
                            Err(e) => {
                                warn!(
                                    version = %vl,
                                    host = %host,
                                    port,
                                    error = %e,
                                    "MQTT event loop error — marking unhealthy"
                                );
                                loop_healthy.store(HEALTH_UNHEALTHY, Ordering::Release);
                                return;
                            }
                        }
                    }
                });

                Ok(MqttConnection {
                    client: MqttClientVariant::V311 { client },
                    health: healthy,
                    last_healthy,
                    reconnect_state: tokio::sync::Mutex::new(ReconnectState {
                        backoff_iter: self.config.reconnect_backoff.build(),
                        next_attempt_at: Instant::now(),
                    }),
                    _event_loop: task,
                })
            }
        }
    }

    /// Gracefully disconnect all pooled connections.
    pub async fn shutdown_all(&self) {
        for entry in self.connections.iter() {
            match &entry.value().client {
                MqttClientVariant::V5 { client } => {
                    let _ = client.disconnect().await;
                }
                MqttClientVariant::V311 { client } => {
                    let _ = client.disconnect().await;
                }
            }
        }
        self.connections.clear();
    }
}

/// Per-endpoint MQTT transport handle implementing [`NotificationTransport`].
pub struct MqttTransportHandle {
    pool: Arc<MqttTransportPool>,
    config: MqttEndpointConfig,
}

/// Build the MQTT notification envelope: `{"metadata": {...}, "body": ...}`.
///
/// The metadata object carries the same information that HTTP notifications
/// transport via headers (Content-Type, Link, receiverInfo key-value pairs,
/// tenant). Used by both V5 and V3.1.1 paths.
fn build_envelope(body: &[u8], content_type: &str, receiver_info: &[(String, String)], context_url: Option<&str>, tenant: Option<&str>) -> Vec<u8> {
    let body_value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);

    let mut metadata = serde_json::Map::new();
    metadata.insert("Content-Type".to_string(), Value::String(content_type.to_string()));

    if content_type == "application/json" {
        let ctx_url = context_url.unwrap_or(crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_URL);
        let link = Link::jsonld_context(ctx_url);
        if let Some(value) = link.to_header_string() {
            metadata.insert("Link".to_string(), Value::String(value));
        }
    }

    for (k, v) in receiver_info {
        metadata.insert(k.clone(), Value::String(v.clone()));
    }

    if let Some(tenant) = tenant {
        metadata.insert("NGSILD-Tenant".to_string(), Value::String(tenant.to_string()));
    }

    let envelope = serde_json::json!({
        "metadata": Value::Object(metadata),
        "body": body_value,
    });

    match serde_json::to_vec(&envelope) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(error = %e, "failed to serialize MQTT notification envelope");
            Vec::new()
        }
    }
}

#[async_trait]
impl NotificationTransport for MqttTransportHandle {
    async fn deliver(&self, req: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        let conn = self.pool.get_or_create_connection(&self.config).await?;
        let content_type_str = req.content_type.as_str();
        let start = std::time::Instant::now();

        let qos = &self.config.qos;

        let envelope = build_envelope(
            req.payload.as_ref(),
            content_type_str,
            &req.receiver_info,
            req.context_url.as_deref(),
            req.tenant.as_deref(),
        );
        let mqtt_endpoint = format!("{}:{}/{}", self.config.host, self.config.port, self.config.topic);

        match &conn.client {
            MqttClientVariant::V5 { client } => {
                let mut properties = rumqttc::v5::mqttbytes::v5::PublishProperties {
                    content_type: Some(content_type_str.to_string()),
                    ..Default::default()
                };

                for (k, v) in &req.receiver_info {
                    properties.user_properties.push((k.clone(), v.clone()));
                }

                if req.content_type == crate::http::model::response_format::APPLICATION_JSON {
                    let ctx_url = req.context_url.as_deref().unwrap_or(crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_URL);
                    let link = Link::jsonld_context(ctx_url);
                    if let Some(value) = link.to_header_string() {
                        properties.user_properties.push(("Link".to_string(), value));
                    }
                }

                if let Some(ref tenant) = req.tenant {
                    properties.user_properties.push(("NGSILD-Tenant".to_string(), tenant.clone()));
                }

                let v5_qos = match qos {
                    MqttQoS::AtMost => rumqttc::v5::mqttbytes::QoS::AtMostOnce,
                    MqttQoS::AtLeast => rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
                    MqttQoS::Exactly => rumqttc::v5::mqttbytes::QoS::ExactlyOnce,
                };

                // V5: envelope body + user properties for metadata.
                client
                    .publish_with_properties(&self.config.topic, v5_qos, false, envelope, properties)
                    .await
                    .map_err(|e| {
                        conn.mark_unhealthy();
                        TransportError::ConnectionFailed {
                            endpoint: mqtt_endpoint,
                            detail: e.to_string(),
                        }
                    })?;
            }
            MqttClientVariant::V311 { client } => {
                let v4_qos = match qos {
                    MqttQoS::AtMost => rumqttc::QoS::AtMostOnce,
                    MqttQoS::AtLeast => rumqttc::QoS::AtLeastOnce,
                    MqttQoS::Exactly => rumqttc::QoS::ExactlyOnce,
                };

                // V3.1.1: envelope only (no user properties available).
                client.publish(&self.config.topic, v4_qos, false, envelope).await.map_err(|e| {
                    conn.mark_unhealthy();
                    TransportError::ConnectionFailed {
                        endpoint: mqtt_endpoint,
                        detail: e.to_string(),
                    }
                })?;
            }
        }

        conn.mark_healthy();
        let duration = start.elapsed();

        info!(
            version = %self.config.version,
            host = %self.config.host,
            port = self.config.port,
            topic = %self.config.topic,
            "MQTT publish succeeded"
        );

        Ok(DeliveryOutcome::Success { status_code: None, duration })
    }

    async fn shutdown(&self) {
        let key = MqttConnectionKey::from_config(&self.config);
        if let Some((_, conn)) = self.pool.connections.remove(&key) {
            match &conn.client {
                MqttClientVariant::V5 { client } => {
                    let _ = client.disconnect().await;
                }
                MqttClientVariant::V311 { client } => {
                    let _ = client.disconnect().await;
                }
            }
        }
    }
}
