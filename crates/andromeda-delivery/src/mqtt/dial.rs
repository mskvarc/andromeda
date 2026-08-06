//! Opening a broker connection, in whichever protocol version it speaks.
//!
//! The two halves of this module are the same sequence written against two
//! `rumqttc` APIs: build options, connect with a timeout, subscribe to `$SYS`,
//! spawn the event loop. They are not deduplicated because nothing they touch
//! is the same type — options, client, event, packet, and quality of service
//! are all distinct between MQTT 5 and MQTT 3.1.1, and a macro that papered
//! over that would hide which protocol a line belongs to.

use crate::{
    endpoint::mqtt::{MqttEndpointConfig, MqttVersion},
    error::TransportError,
    mqtt::{
        config::MqttPoolConfig,
        connection::{ConnectionState, MqttClientVariant},
        connection_key::MqttConnectionKey,
        sys_topics::SYS_TOPICS,
    },
};
use rumqttc::{
    AsyncClient,
    Event,
    MqttOptions,
    Packet,
    QoS,
    Transport,
    v5::{
        AsyncClient as AsyncClientV5,
        Event as EventV5,
        MqttOptions as MqttOptionsV5,
        mqttbytes::{QoS as QoSV5, v5::Packet as PacketV5},
    },
};
use secrecy::ExposeSecret;
use std::{str, sync::Arc};
use tokio::{task::JoinHandle, time::timeout};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Inbound queue depth for a broker connection's event loop.
const EVENT_LOOP_CAPACITY: usize = 64;

/// The broker a log line is about.
///
/// Carried into the spawned event loop, which outlives every borrow of the
/// endpoint it came from.
#[derive(Debug, Clone)]
struct BrokerLabel {
    /// Broker hostname.
    host: String,
    /// Broker port.
    port: u16,
    /// The protocol version in use.
    version: MqttVersion,
}

/// Open a connection to the broker the key names.
pub async fn dial(
    key: &MqttConnectionKey,
    endpoint: &MqttEndpointConfig,
    pool: &MqttPoolConfig,
    state: &Arc<ConnectionState>,
) -> Result<(MqttClientVariant, JoinHandle<()>), TransportError> {
    let client_id = format!("andromeda-{}", Uuid::new_v4());
    let label = BrokerLabel {
        host: key.host.clone(), // clone: the label outlives this borrow of the key
        port: key.port,
        version: key.version,
    };

    match key.version {
        MqttVersion::V5 => dial_v5(&client_id, key, endpoint, pool, state, label).await,
        MqttVersion::V311 => dial_v311(&client_id, key, endpoint, pool, state, label).await,
    }
}

/// The failure a connect attempt that did not complete becomes.
fn connect_failed(label: &BrokerLabel, detail: String) -> TransportError {
    TransportError::ConnectionFailed {
        endpoint: format!("{}:{}", label.host, label.port),
        detail: format!("MQTT {} connection failed: {detail}", label.version),
    }
}

/// The failure a connect attempt that ran out of time becomes.
fn connect_timed_out(label: &BrokerLabel, pool: &MqttPoolConfig) -> TransportError {
    TransportError::Timeout {
        endpoint: format!("{}:{}", label.host, label.port),
        duration: pool.connect_timeout,
    }
}

/// Open an MQTT 5 connection.
async fn dial_v5(
    client_id: &str,
    key: &MqttConnectionKey,
    endpoint: &MqttEndpointConfig,
    pool: &MqttPoolConfig,
    state: &Arc<ConnectionState>,
    label: BrokerLabel,
) -> Result<(MqttClientVariant, JoinHandle<()>), TransportError> {
    let mut options = MqttOptionsV5::new(client_id, &key.host, key.port);
    options.set_keep_alive(pool.keepalive);
    if key.use_tls {
        options.set_transport(Transport::tls_with_default_config());
    }
    if let Some(username) = endpoint.username.as_ref() {
        let password = endpoint.password.as_ref().map_or_else(String::new, |secret| secret.expose_secret().to_owned());
        options.set_credentials(username.clone(), password); // clone: `set_credentials` takes ownership
    }

    let (client, mut eventloop) = AsyncClientV5::new(options, EVENT_LOOP_CAPACITY);

    match timeout(pool.connect_timeout, eventloop.poll()).await {
        Ok(Ok(_connected)) => info!(version = %label.version, host = %label.host, port = label.port, "MQTT connected"),
        Ok(Err(error)) => return Err(connect_failed(&label, error.to_string())),
        Err(_elapsed) => return Err(connect_timed_out(&label, pool)),
    }

    for topic in SYS_TOPICS {
        if let Err(error) = client.subscribe(*topic, QoSV5::AtMostOnce).await {
            debug!(topic, error = %error, "failed to subscribe to $SYS topic");
        }
    }

    let loop_state = Arc::clone(state);
    let task = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(event) => {
                    loop_state.mark_healthy();
                    if let EventV5::Incoming(PacketV5::Publish(publish)) = &event
                        && let Ok(topic) = str::from_utf8(&publish.topic)
                        && topic.starts_with("$SYS/")
                    {
                        let payload = String::from_utf8_lossy(&publish.payload).into_owned();
                        loop_state.sys_data.write().await.record(topic, payload);
                    }
                }
                Err(error) => {
                    warn!(
                        version = %label.version,
                        host = %label.host,
                        port = label.port,
                        error = %error,
                        "MQTT event loop error — marking unhealthy"
                    );
                    loop_state.mark_unhealthy();
                    return;
                }
            }
        }
    });

    Ok((MqttClientVariant::V5 { client }, task))
}

/// Open an MQTT 3.1.1 connection.
async fn dial_v311(
    client_id: &str,
    key: &MqttConnectionKey,
    endpoint: &MqttEndpointConfig,
    pool: &MqttPoolConfig,
    state: &Arc<ConnectionState>,
    label: BrokerLabel,
) -> Result<(MqttClientVariant, JoinHandle<()>), TransportError> {
    let mut options = MqttOptions::new(client_id, &key.host, key.port);
    options.set_keep_alive(pool.keepalive);
    if key.use_tls {
        options.set_transport(Transport::tls_with_default_config());
    }
    if let Some(username) = endpoint.username.as_ref() {
        let password = endpoint.password.as_ref().map_or_else(String::new, |secret| secret.expose_secret().to_owned());
        options.set_credentials(username.clone(), password); // clone: `set_credentials` takes ownership
    }

    let (client, mut eventloop) = AsyncClient::new(options, EVENT_LOOP_CAPACITY);

    match timeout(pool.connect_timeout, eventloop.poll()).await {
        Ok(Ok(_connected)) => info!(version = %label.version, host = %label.host, port = label.port, "MQTT connected"),
        Ok(Err(error)) => return Err(connect_failed(&label, error.to_string())),
        Err(_elapsed) => return Err(connect_timed_out(&label, pool)),
    }

    for topic in SYS_TOPICS {
        if let Err(error) = client.subscribe(*topic, QoS::AtMostOnce).await {
            debug!(topic, error = %error, "failed to subscribe to $SYS topic");
        }
    }

    let loop_state = Arc::clone(state);
    let task = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(event) => {
                    loop_state.mark_healthy();
                    if let Event::Incoming(Packet::Publish(publish)) = &event
                        && publish.topic.starts_with("$SYS/")
                    {
                        let payload = String::from_utf8_lossy(&publish.payload).into_owned();
                        loop_state.sys_data.write().await.record(&publish.topic, payload);
                    }
                }
                Err(error) => {
                    warn!(
                        version = %label.version,
                        host = %label.host,
                        port = label.port,
                        error = %error,
                        "MQTT event loop error — marking unhealthy"
                    );
                    loop_state.mark_unhealthy();
                    return;
                }
            }
        }
    });

    Ok((MqttClientVariant::V311 { client }, task))
}
