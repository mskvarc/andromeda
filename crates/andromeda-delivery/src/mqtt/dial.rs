//! Opening a broker connection, in whichever protocol version it speaks.
//!
//! The two halves of this module are the same sequence written against two
//! `rumqttc` APIs: build options, connect with a timeout, subscribe to `$SYS`,
//! spawn the event loop. They are not deduplicated because nothing they touch
//! is the same type: options, client, event, packet, and quality of service
//! are all distinct between MQTT 5 and MQTT 3.1.1, and a macro that papered
//! over that would hide which protocol a line belongs to.

use crate::{
    connection_failure::ConnectionFailure,
    endpoint::mqtt::{MqttEndpointConfig, MqttVersion},
    error::TransportError,
    mqtt::{
        config::MqttPoolConfig,
        connection::{ConnectionState, MqttClientVariant},
        connection_key::MqttConnectionKey,
        packet_ceiling::MqttPacketCeiling,
        sys_topics::SysTopic,
    },
};
use rumqttc::{
    AsyncClient,
    Event,
    MqttOptions,
    Outgoing,
    Packet,
    QoS,
    TlsConfiguration,
    Transport,
    v5::{
        AsyncClient as AsyncClientV5,
        Event as EventV5,
        MqttOptions as MqttOptionsV5,
        mqttbytes::{QoS as QoSV5, v5::Packet as PacketV5},
    },
};
use rustls::ClientConfig;
use secrecy::ExposeSecret;
use std::{future::Future, pin::Pin, str, sync::Arc};
use strum::IntoEnumIterator;
use tokio::{task::JoinHandle, time::timeout};
use tracing::{debug, info, warn};
use url::Url;
use uuid::Uuid;

/// Inbound queue depth for a broker connection's event loop.
const EVENT_LOOP_CAPACITY: usize = 64;

/// The asynchronous result of opening one MQTT session.
pub(crate) type DialFuture<'a> = Pin<Box<dyn Future<Output = Result<(MqttClientVariant, JoinHandle<()>), TransportError>> + Send + 'a>>;

/// Capability for opening one MQTT transport connection.
///
/// The pool owns this narrow boundary so its lifecycle policy is testable
/// without making a socket connection. Production uses [`RumqttcDialer`].
pub trait MqttDialer: Send + Sync {
    /// Open a broker connection for this key and state holder.
    fn dial<'a>(
        &'a self,
        key: &'a MqttConnectionKey,
        endpoint: &'a MqttEndpointConfig,
        pool: &'a MqttPoolConfig,
        state: &'a Arc<ConnectionState>,
    ) -> DialFuture<'a>;
}

/// The production dialer backed by `rumqttc`.
#[derive(Debug)]
pub struct RumqttcDialer {
    /// The TLS client configuration every `mqtts` connection is opened with.
    tls: Arc<ClientConfig>,
}

impl RumqttcDialer {
    /// A dialer that opens every `mqtts` connection with `tls`.
    #[must_use]
    pub const fn new(tls: Arc<ClientConfig>) -> RumqttcDialer {
        RumqttcDialer { tls }
    }
}

impl MqttDialer for RumqttcDialer {
    fn dial<'a>(
        &'a self,
        key: &'a MqttConnectionKey,
        endpoint: &'a MqttEndpointConfig,
        pool: &'a MqttPoolConfig,
        state: &'a Arc<ConnectionState>,
    ) -> DialFuture<'a> {
        Box::pin(dial(key, endpoint, pool, state, &self.tls))
    }
}

/// The broker a log line or error is about.
///
/// Carried into the spawned event loop, which outlives every borrow of the
/// endpoint it came from. Holds the credential-free endpoint URI so a failure it
/// becomes names the endpoint without leaking the broker password.
#[derive(Debug, Clone)]
struct BrokerLabel {
    /// The endpoint URI, with any credentials already stripped.
    endpoint_uri: Url,
    /// The protocol version in use.
    version: MqttVersion,
}

/// Open a connection to the broker the key names, over TLS configured by `tls`
/// when the key asks for it.
///
/// # Errors
///
/// Returns [`TransportError::Timeout`] when the broker does not complete the
/// connect handshake within the pool's connect timeout, and
/// [`TransportError::ConnectionFailed`] when the handshake itself fails: the
/// socket is refused, TLS is rejected, or the broker denies the credentials.
/// Failing to subscribe to a `$SYS` topic is not an error: the connection is
/// usable without the health telemetry those topics carry.
pub async fn dial(
    key: &MqttConnectionKey,
    endpoint: &MqttEndpointConfig,
    pool: &MqttPoolConfig,
    state: &Arc<ConnectionState>,
    tls: &Arc<ClientConfig>,
) -> Result<(MqttClientVariant, JoinHandle<()>), TransportError> {
    let client_id = format!("andromeda-{}", Uuid::new_v4());
    let label = BrokerLabel {
        endpoint_uri: endpoint.endpoint_uri.clone(), // clone: the label outlives this borrow of the endpoint
        version: key.version,
    };
    let transport = transport_for(key, tls);

    match key.version {
        MqttVersion::V5 => dial_v5(&client_id, key, endpoint, pool, state, transport, label).await,
        MqttVersion::V311 => dial_v311(&client_id, key, endpoint, pool, state, transport, label).await,
    }
}

/// The transport a connection to the broker the key names runs over.
fn transport_for(key: &MqttConnectionKey, tls: &Arc<ClientConfig>) -> Transport {
    if key.use_tls {
        Transport::tls_with_config(TlsConfiguration::Rustls(Arc::clone(tls)))
    } else {
        Transport::Tcp
    }
}

/// The failure a connect attempt that did not complete becomes.
fn connect_failed(label: &BrokerLabel, cause: ConnectionFailure) -> TransportError {
    TransportError::ConnectionFailed {
        endpoint: label.endpoint_uri.clone(), // clone: the error owns the endpoint label it names
        cause,
    }
}

/// The failure a connect attempt that ran out of time becomes.
fn connect_timed_out(label: &BrokerLabel, pool: &MqttPoolConfig) -> TransportError {
    TransportError::Timeout {
        endpoint: label.endpoint_uri.clone(),
        duration: pool.connect_timeout,
    }
}

/// The largest packet an MQTT 5 server announced it accepts, carried by the
/// CONNACK that opens the session (MQTT 5.0 section 3.2.2.3.6).
fn announced_max_packet_size(connected: &EventV5) -> Option<u32> {
    if let EventV5::Incoming(PacketV5::ConnAck(connack)) = connected {
        connack.properties.as_ref().and_then(|properties| properties.max_packet_size)
    } else {
        None
    }
}

/// Open an MQTT 5 connection.
async fn dial_v5(
    client_id: &str,
    key: &MqttConnectionKey,
    endpoint: &MqttEndpointConfig,
    pool: &MqttPoolConfig,
    state: &Arc<ConnectionState>,
    transport: Transport,
    label: BrokerLabel,
) -> Result<(MqttClientVariant, JoinHandle<()>), TransportError> {
    let mut options = MqttOptionsV5::new(client_id, key.authority.host_str(), key.authority.port());
    options.set_keep_alive(pool.keepalive);
    options.set_max_packet_size(Some(pool.max_packet_size.octets()));
    options.set_transport(transport);
    if let Some(username) = endpoint.username.as_ref() {
        let password = endpoint.password.as_ref().map_or_else(String::new, |secret| secret.expose_secret().to_owned());
        options.set_credentials(username.clone(), password); // clone: `set_credentials` takes ownership
    }

    let (client, mut eventloop) = AsyncClientV5::new(options, EVENT_LOOP_CAPACITY);

    let announced = match timeout(pool.connect_timeout, eventloop.poll()).await {
        Ok(Ok(connected)) => {
            info!(version = %label.version, endpoint = %label.endpoint_uri, "MQTT connected");
            announced_max_packet_size(&connected)
        }
        Ok(Err(error)) => return Err(connect_failed(&label, ConnectionFailure::MqttV5Connect(Box::new(error)))),
        Err(_elapsed) => return Err(connect_timed_out(&label, pool)),
    };
    let packet_ceiling = MqttPacketCeiling::negotiated(pool.max_packet_size, announced);
    if let MqttPacketCeiling::ServerAnnounced { announced, configured } = packet_ceiling {
        info!(
            endpoint = %label.endpoint_uri,
            announced,
            max_mqtt_packet = configured.octets(),
            "the MQTT server announced a maximum packet size below notifications.delivery.max_mqtt_packet, which bounds the notifications this connection publishes"
        );
    }

    for sys in SysTopic::iter() {
        if let Err(error) = client.subscribe(sys.topic(), QoSV5::AtMostOnce).await {
            debug!(topic = sys.topic(), error = %error, "failed to subscribe to $SYS topic");
        }
    }

    let loop_state = Arc::clone(state);
    let task = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(event) => {
                    loop_state.mark_healthy();
                    // Correlate publishes with their acknowledgements, and read
                    // `$SYS`. Every other packet is background traffic the
                    // connection handles itself.
                    if let EventV5::Outgoing(Outgoing::Publish(pkid)) = &event {
                        loop_state.acks().on_outgoing_publish(*pkid);
                    } else if let EventV5::Incoming(PacketV5::PubAck(puback)) = &event {
                        loop_state.acks().on_puback(puback.pkid);
                    } else if let EventV5::Incoming(PacketV5::PubComp(pubcomp)) = &event {
                        loop_state.acks().on_pubcomp(pubcomp.pkid);
                    } else if let EventV5::Incoming(PacketV5::Publish(publish)) = &event
                        && let Ok(topic) = str::from_utf8(&publish.topic)
                        && let Some(sys) = SysTopic::from_topic(topic)
                    {
                        let value = String::from_utf8_lossy(&publish.payload).into_owned();
                        loop_state.sys_data.write().await.record(sys, value);
                    }
                }
                Err(error) => {
                    warn!(
                        version = %label.version,
                        endpoint = %label.endpoint_uri,
                        error = %error,
                        "MQTT event loop error — marking unhealthy"
                    );
                    loop_state.mark_unhealthy();
                    // The connection is gone; every publish awaiting an
                    // acknowledgement on it is now unconfirmed and retryable.
                    loop_state.acks().fail_all();
                    return;
                }
            }
        }
    });

    Ok((MqttClientVariant::V5 { client, packet_ceiling }, task))
}

/// Open an MQTT 3.1.1 connection.
async fn dial_v311(
    client_id: &str,
    key: &MqttConnectionKey,
    endpoint: &MqttEndpointConfig,
    pool: &MqttPoolConfig,
    state: &Arc<ConnectionState>,
    transport: Transport,
    label: BrokerLabel,
) -> Result<(MqttClientVariant, JoinHandle<()>), TransportError> {
    let mut options = MqttOptions::new(client_id, key.authority.host_str(), key.authority.port());
    options.set_keep_alive(pool.keepalive);
    let packet_ceiling = MqttPacketCeiling::Configured(pool.max_packet_size);
    let max_packet_size = pool.max_packet_size.octets() as usize;
    options.set_max_packet_size(max_packet_size, max_packet_size);
    options.set_transport(transport);
    if let Some(username) = endpoint.username.as_ref() {
        let password = endpoint.password.as_ref().map_or_else(String::new, |secret| secret.expose_secret().to_owned());
        options.set_credentials(username.clone(), password); // clone: `set_credentials` takes ownership
    }

    let (client, mut eventloop) = AsyncClient::new(options, EVENT_LOOP_CAPACITY);

    match timeout(pool.connect_timeout, eventloop.poll()).await {
        Ok(Ok(_connected)) => info!(version = %label.version, endpoint = %label.endpoint_uri, "MQTT connected"),
        Ok(Err(error)) => return Err(connect_failed(&label, ConnectionFailure::MqttV311Connect(Box::new(error)))),
        Err(_elapsed) => return Err(connect_timed_out(&label, pool)),
    }

    for sys in SysTopic::iter() {
        if let Err(error) = client.subscribe(sys.topic(), QoS::AtMostOnce).await {
            debug!(topic = sys.topic(), error = %error, "failed to subscribe to $SYS topic");
        }
    }

    let loop_state = Arc::clone(state);
    let task = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(event) => {
                    loop_state.mark_healthy();
                    // Correlate publishes with their acknowledgements, and read
                    // `$SYS`. Every other packet is background traffic the
                    // connection handles itself.
                    if let Event::Outgoing(Outgoing::Publish(pkid)) = &event {
                        loop_state.acks().on_outgoing_publish(*pkid);
                    } else if let Event::Incoming(Packet::PubAck(puback)) = &event {
                        loop_state.acks().on_puback(puback.pkid);
                    } else if let Event::Incoming(Packet::PubComp(pubcomp)) = &event {
                        loop_state.acks().on_pubcomp(pubcomp.pkid);
                    } else if let Event::Incoming(Packet::Publish(publish)) = &event
                        && let Some(sys) = SysTopic::from_topic(&publish.topic)
                    {
                        let value = String::from_utf8_lossy(&publish.payload).into_owned();
                        loop_state.sys_data.write().await.record(sys, value);
                    }
                }
                Err(error) => {
                    warn!(
                        version = %label.version,
                        endpoint = %label.endpoint_uri,
                        error = %error,
                        "MQTT event loop error — marking unhealthy"
                    );
                    loop_state.mark_unhealthy();
                    // The connection is gone; every publish awaiting an
                    // acknowledgement on it is now unconfirmed and retryable.
                    loop_state.acks().fail_all();
                    return;
                }
            }
        }
    });

    Ok((MqttClientVariant::V311 { client, packet_ceiling }, task))
}

#[cfg(test)]
mod tests {
    use crate::{
        connection_failure::ConnectionFailure,
        endpoint::{
            mqtt::{MqttEndpointConfig, MqttQoS, MqttVersion},
            topic::MqttTopic,
        },
        error::TransportError,
        host_port::HostPort,
        mqtt::{client_tls::mqtt_client_tls, config::MqttPoolConfig, connection::ConnectionState, connection_key::MqttConnectionKey, dial::dial},
    };
    use rumqttc::{ConnectionError, v5::ConnectionError as ConnectionErrorV5};
    use std::{str::FromStr, sync::Arc};
    use tokio::net::TcpListener;
    use url::Url;

    /// A loopback peer that accepts every TCP connection and closes it before
    /// sending a TLS record.
    async fn handshake_dropping_peer() -> HostPort {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port binds");
        let port = listener.local_addr().expect("the listener has an address").port();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });
        HostPort::from_str(&format!("127.0.0.1:{port}")).expect("a valid authority")
    }

    /// An `mqtts` endpoint on `authority`, speaking `version`.
    fn tls_endpoint(authority: HostPort, version: MqttVersion) -> MqttEndpointConfig {
        MqttEndpointConfig {
            endpoint_uri: Url::parse(&format!("mqtts://{authority}/andromeda/test")).expect("a valid endpoint URI"),
            authority,
            topic: MqttTopic::from_str("andromeda/test").expect("a valid topic"),
            use_tls: true,
            version,
            qos: MqttQoS::AtMost,
            username: None,
            password: None,
        }
    }

    #[tokio::test]
    async fn an_mqtts_dial_whose_handshake_is_cut_fails_on_the_tls_layer_in_both_protocol_versions() {
        let tls = mqtt_client_tls().expect("the system trust store yields a TLS client configuration");
        for version in [MqttVersion::V311, MqttVersion::V5] {
            let endpoint = tls_endpoint(handshake_dropping_peer().await, version);
            let key = MqttConnectionKey::from_config(&endpoint);
            let state = Arc::new(ConnectionState::connected());

            let failure = dial(&key, &endpoint, &MqttPoolConfig::default(), &state, &tls)
                .await
                .map(drop)
                .expect_err("the peer never completes a TLS handshake");

            assert!(
                matches!(
                    &failure,
                    TransportError::ConnectionFailed {
                        cause: ConnectionFailure::MqttV311Connect(cause),
                        ..
                    } if matches!(**cause, ConnectionError::Tls(_))
                ) || matches!(
                    &failure,
                    TransportError::ConnectionFailed {
                        cause: ConnectionFailure::MqttV5Connect(cause),
                        ..
                    } if matches!(**cause, ConnectionErrorV5::Tls(_))
                ),
                "{version}: {failure:?}"
            );
        }
    }
}
