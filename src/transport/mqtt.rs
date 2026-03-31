use crate::{
    api::header::{jsonld_context_link, link_header_string},
    domain::notification::{MqttEndpointConfig, MqttQoS, MqttVersion},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct BrokerKey {
    host: String,
    port: u16,
    use_tls: bool,
    version: MqttVersion,
    username: Option<String>,
}

struct MqttClientHandle {
    healthy: Arc<AtomicBool>,
    inner: MqttClientInner,
}

enum MqttClientInner {
    V5 {
        client: rumqttc::v5::AsyncClient,
        _event_loop: tokio::task::JoinHandle<()>,
    },
    V4 {
        client: rumqttc::AsyncClient,
        _event_loop: tokio::task::JoinHandle<()>,
    },
}

pub struct MqttConnectionManager {
    connections: Arc<RwLock<HashMap<BrokerKey, Arc<MqttClientHandle>>>>,
}

/// Build the MQTT notification envelope: `{"metadata": {...}, "body": ...}`.
///
/// The metadata object carries the same information that HTTP notifications
/// transport via headers (Content-Type, Link, receiverInfo key-value pairs).
fn build_envelope(body: &[u8], content_type: &str, receiver_info: Option<&[Value]>, jsonld_context: Option<&str>) -> Vec<u8> {
    let body_value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);

    let mut metadata = serde_json::Map::new();
    metadata.insert("Content-Type".to_string(), Value::String(content_type.to_string()));

    if content_type == "application/json" {
        let ctx_url = jsonld_context.unwrap_or(crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_URL);
        let link = jsonld_context_link(ctx_url);
        if let Some(value) = link_header_string(&link) {
            metadata.insert("Link".to_string(), Value::String(value));
        }
    }

    if let Some(infos) = receiver_info {
        for info in infos {
            if let (Some(k), Some(v)) = (info.get("key").and_then(|v| v.as_str()), info.get("value").and_then(|v| v.as_str())) {
                metadata.insert(k.to_string(), Value::String(v.to_string()));
            }
        }
    }

    let envelope = serde_json::json!({
        "metadata": Value::Object(metadata),
        "body": body_value,
    });

    serde_json::to_vec(&envelope).unwrap_or_default()
}

fn with_tenant_metadata(mut envelope: Vec<u8>, tenant: Option<&str>) -> Vec<u8> {
    let Some(tenant) = tenant else {
        return envelope;
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(&envelope) else {
        return envelope;
    };
    if let Some(metadata) = value.get_mut("metadata").and_then(|v| v.as_object_mut()) {
        metadata.insert("NGSILD-Tenant".to_string(), Value::String(tenant.to_string()));
    }
    envelope = serde_json::to_vec(&value).unwrap_or(envelope);
    envelope
}

impl MqttConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn publish(
        &self,
        config: &MqttEndpointConfig,
        payload: &[u8],
        content_type: &str,
        receiver_info: Option<&[Value]>,
        jsonld_context: Option<&str>,
        tenant: Option<&str>,
    ) -> bool {
        let key = BrokerKey {
            host: config.host.clone(),
            port: config.port,
            use_tls: config.use_tls,
            version: config.version,
            username: config.username.clone(),
        };

        // Check for a cached connection, but discard it if unhealthy.
        let handle = {
            let conns = self.connections.read().await;
            match conns.get(&key) {
                Some(h) if h.healthy.load(Ordering::Acquire) => Some(h.clone()),
                Some(_) => None, // stale — will be removed below
                None => None,
            }
        };

        // Remove stale entry outside the read lock.
        let handle = match handle {
            Some(h) => h,
            None => {
                // Remove any unhealthy entry before creating a fresh one.
                self.connections.write().await.remove(&key);
                match self.create_connection(&key, config).await {
                    Ok(h) => h,
                    Err(e) => {
                        error!("Failed to create MQTT connection to {}:{}: {}", key.host, key.port, e);
                        return false;
                    }
                }
            }
        };

        let envelope = with_tenant_metadata(build_envelope(payload, content_type, receiver_info, jsonld_context), tenant);

        let qos = match config.qos {
            MqttQoS::AtMost => 0,
            MqttQoS::AtLeast => 1,
            MqttQoS::Exactly => 2,
        };

        match &handle.inner {
            MqttClientInner::V5 { client, .. } => {
                let mut properties = rumqttc::v5::mqttbytes::v5::PublishProperties {
                    content_type: Some(content_type.to_string()),
                    ..Default::default()
                };

                if let Some(infos) = receiver_info {
                    for info in infos {
                        if let (Some(k), Some(v)) = (info.get("key").and_then(|v| v.as_str()), info.get("value").and_then(|v| v.as_str())) {
                            properties.user_properties.push((k.to_string(), v.to_string()));
                        }
                    }
                }

                if content_type == "application/json" {
                    let ctx_url = jsonld_context.unwrap_or(crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_URL);
                    let link = jsonld_context_link(ctx_url);
                    if let Some(value) = link_header_string(&link) {
                        properties.user_properties.push(("Link".to_string(), value));
                    }
                }
                if let Some(tenant) = tenant {
                    properties.user_properties.push(("NGSILD-Tenant".to_string(), tenant.to_string()));
                }

                let v5_qos = match qos {
                    0 => rumqttc::v5::mqttbytes::QoS::AtMostOnce,
                    1 => rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
                    _ => rumqttc::v5::mqttbytes::QoS::ExactlyOnce,
                };

                match client.publish_with_properties(&config.topic, v5_qos, false, envelope, properties).await {
                    Ok(()) => {
                        info!("MQTT v5 publish to {}:{}/{} succeeded", config.host, config.port, config.topic);
                        true
                    }
                    Err(e) => {
                        warn!("MQTT v5 publish to {}:{}/{} failed: {}", config.host, config.port, config.topic, e);
                        handle.healthy.store(false, Ordering::Release);
                        self.connections.write().await.remove(&key);
                        false
                    }
                }
            }
            MqttClientInner::V4 { client, .. } => {
                let v4_qos = match qos {
                    0 => rumqttc::QoS::AtMostOnce,
                    1 => rumqttc::QoS::AtLeastOnce,
                    _ => rumqttc::QoS::ExactlyOnce,
                };

                match client.publish(&config.topic, v4_qos, false, envelope).await {
                    Ok(()) => {
                        info!("MQTT v3.1.1 publish to {}:{}/{} succeeded", config.host, config.port, config.topic);
                        true
                    }
                    Err(e) => {
                        warn!("MQTT v3.1.1 publish to {}:{}/{} failed: {}", config.host, config.port, config.topic, e);
                        handle.healthy.store(false, Ordering::Release);
                        self.connections.write().await.remove(&key);
                        false
                    }
                }
            }
        }
    }

    async fn create_connection(&self, key: &BrokerKey, config: &MqttEndpointConfig) -> Result<Arc<MqttClientHandle>, String> {
        let mut conns = self.connections.write().await;

        // Double-check after acquiring write lock
        if let Some(existing) = conns.get(key) {
            if existing.healthy.load(Ordering::Acquire) {
                return Ok(existing.clone());
            }
            conns.remove(key);
        }

        let client_id = format!("andromeda-{}", uuid::Uuid::new_v4());
        let healthy = Arc::new(AtomicBool::new(true));

        let version_label = match key.version {
            MqttVersion::V5 => "v5",
            MqttVersion::V311 => "v3.1.1",
        };
        let connect_timeout = std::time::Duration::from_secs(5);
        let keep_alive = std::time::Duration::from_secs(30);

        let handle = match key.version {
            MqttVersion::V5 => {
                let mut options = rumqttc::v5::MqttOptions::new(&client_id, &key.host, key.port);
                options.set_keep_alive(keep_alive);
                if key.use_tls {
                    options.set_transport(rumqttc::Transport::tls_with_default_config());
                }
                if let Some(ref username) = config.username {
                    options.set_credentials(username.clone(), config.password.clone().unwrap_or_default());
                }

                let (client, mut eventloop) = rumqttc::v5::AsyncClient::new(options, 64);

                match tokio::time::timeout(connect_timeout, eventloop.poll()).await {
                    Ok(Ok(_)) => info!("MQTT {} connected to {}:{}", version_label, key.host, key.port),
                    Ok(Err(e)) => return Err(format!("MQTT {} connection to {}:{} failed: {}", version_label, key.host, key.port, e)),
                    Err(_) => return Err(format!("MQTT {} connection to {}:{} timed out", version_label, key.host, key.port)),
                }

                let host = key.host.clone();
                let port = key.port;
                let loop_healthy = healthy.clone();
                let vl = version_label;
                let task = tokio::spawn(async move {
                    loop {
                        match eventloop.poll().await {
                            Ok(_) => {}
                            Err(e) => {
                                warn!("MQTT {} event loop error ({}:{}): {} — marking unhealthy", vl, host, port, e);
                                loop_healthy.store(false, Ordering::Release);
                                return;
                            }
                        }
                    }
                });

                Arc::new(MqttClientHandle {
                    healthy: healthy.clone(),
                    inner: MqttClientInner::V5 { client, _event_loop: task },
                })
            }
            MqttVersion::V311 => {
                let mut options = rumqttc::MqttOptions::new(&client_id, &key.host, key.port);
                options.set_keep_alive(keep_alive);
                if key.use_tls {
                    options.set_transport(rumqttc::Transport::tls_with_default_config());
                }
                if let Some(ref username) = config.username {
                    options.set_credentials(username.clone(), config.password.clone().unwrap_or_default());
                }

                let (client, mut eventloop) = rumqttc::AsyncClient::new(options, 64);

                match tokio::time::timeout(connect_timeout, eventloop.poll()).await {
                    Ok(Ok(_)) => info!("MQTT {} connected to {}:{}", version_label, key.host, key.port),
                    Ok(Err(e)) => return Err(format!("MQTT {} connection to {}:{} failed: {}", version_label, key.host, key.port, e)),
                    Err(_) => return Err(format!("MQTT {} connection to {}:{} timed out", version_label, key.host, key.port)),
                }

                let host = key.host.clone();
                let port = key.port;
                let loop_healthy = healthy.clone();
                let vl = version_label;
                let task = tokio::spawn(async move {
                    loop {
                        match eventloop.poll().await {
                            Ok(_) => {}
                            Err(e) => {
                                warn!("MQTT {} event loop error ({}:{}): {} — marking unhealthy", vl, host, port, e);
                                loop_healthy.store(false, Ordering::Release);
                                return;
                            }
                        }
                    }
                });

                Arc::new(MqttClientHandle {
                    healthy: healthy.clone(),
                    inner: MqttClientInner::V4 { client, _event_loop: task },
                })
            }
        };

        conns.insert(key.clone(), handle.clone());
        Ok(handle)
    }
}
