//! Publishing one notification to one MQTT endpoint.

use crate::{
    connection_failure::ConnectionFailure,
    delivery_deadline::delivery_deadline,
    endpoint::mqtt::{MqttEndpointConfig, MqttQoS},
    error::TransportError,
    mqtt::{ack::AckKind, envelope::build_envelope, outgoing_publish::OutgoingPublish, pool::MqttTransportPool},
    outcome::{DeliveryOutcome, delivery_metric_outcome},
    request::DeliveryRequest,
    transport::NotificationTransport,
};
use andromeda_telemetry::delivery_ops::DeliveryTimer;
use async_trait::async_trait;
use std::{sync::Arc, time::Instant};
use tokio::time::timeout;
use tracing::info;

/// Per-endpoint MQTT transport handle.
///
/// Holds the endpoint rather than a connection: which connection serves it is
/// the pool's decision and may change between deliveries.
pub struct MqttTransportHandle {
    /// The pool the connection is drawn from.
    pool: Arc<MqttTransportPool>,
    /// The endpoint this handle publishes to.
    config: MqttEndpointConfig,
}

impl MqttTransportHandle {
    /// Bind a handle to one endpoint on a pool.
    #[must_use]
    pub const fn new(pool: Arc<MqttTransportPool>, config: MqttEndpointConfig) -> MqttTransportHandle {
        MqttTransportHandle { pool, config }
    }

    /// What completes a publish at this endpoint's `QoS`.
    const fn completing_ack(&self) -> AckKind {
        match self.config.qos {
            MqttQoS::AtMost => AckKind::Written,
            MqttQoS::AtLeast => AckKind::PubAck,
            MqttQoS::Exactly => AckKind::PubComp,
        }
    }

    /// A successful delivery outcome, logged.
    fn succeeded(&self, start: Instant) -> DeliveryOutcome {
        info!(
            version = %self.config.version,
            endpoint = %self.config.endpoint_uri,
            topic = %self.config.topic,
            "MQTT publish succeeded"
        );
        DeliveryOutcome::Success {
            status_code: None,
            duration: start.elapsed(),
        }
    }
}

#[async_trait]
impl NotificationTransport for MqttTransportHandle {
    async fn deliver(&self, request: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        // One delivery attempt, timed and counted as a whole; for MQTT the
        // measured span is connection acquisition, enqueue and acknowledgement.
        let timer = DeliveryTimer::start();
        let result = self.deliver_inner(request).await;
        timer.finish(delivery_metric_outcome(&result));
        result
    }
}

impl MqttTransportHandle {
    /// The delivery body itself, wrapped by [`MqttTransportHandle::deliver`] for timing.
    async fn deliver_inner(&self, request: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        self.pool.check_broker(&self.config).await?;
        let connection = self.pool.get_or_create_connection(&self.config).await?;
        let start = Instant::now();
        let envelope = build_envelope(request)?;
        let outgoing = OutgoingPublish::admitted(&connection.client, &self.config.topic, self.config.qos, request, envelope)?;
        let completes_on = self.completing_ack();

        // Register the waiter and enqueue the publish under one lock, so the
        // event loop meets waiters in the order it writes their packets.
        let publish_guard = connection.state.publish_lock.lock().await;
        let completion = connection.state.acks().register(completes_on);
        let published = outgoing.publish(&self.config.topic).await;
        drop(publish_guard);

        if let Err(cause) = published {
            // The packet never reached the event loop, so its waiter never
            // will; drop it before it desynchronises the tracker.
            connection.state.acks().cancel_last_registered();
            connection.state.mark_unhealthy();
            return Err(TransportError::ConnectionFailed {
                endpoint: self.config.endpoint_uri.clone(), // clone: the error owns the endpoint it names
                cause,
            });
        }

        // QoS 0 is never acknowledged, so it is delivered once the client has
        // written it; a packet the client refuses to write ends the
        // connection, which fails the waiter instead. Table 5.2.15-1 confines
        // the endpoint's `timeout` to a binding whose "protocol always returns
        // a response", which QoS 0 is not, so only the broker-wide window
        // bounds the wait.
        //
        // QoS 1/2 is delivered only once the broker acknowledges it (PUBACK /
        // PUBCOMP). Until then it must not be reported delivered, or an unacked
        // packet the event loop later drops would never retransmit. Here MQTT
        // does always respond, so the endpoint's deadline is honoured under the
        // rule the HTTP binding follows: "the NGSI-LD system can override this
        // value", and the pool's acknowledgement window is that system value.
        let (deadline, lost) = match completes_on {
            AckKind::Written => (self.pool.publish_ack_timeout(), ConnectionFailure::PublishNotWritten),
            AckKind::PubAck | AckKind::PubComp => (
                delivery_deadline(request.timeout, self.pool.publish_ack_timeout()),
                ConnectionFailure::AcknowledgementLost,
            ),
        };
        match timeout(deadline, completion).await {
            Ok(Ok(())) => Ok(self.succeeded(start)),
            Ok(Err(_cancelled)) => Err(TransportError::ConnectionFailed {
                endpoint: self.config.endpoint_uri.clone(), // clone: the error owns the endpoint it names
                cause: lost,
            }),
            // The publish did not complete within the allowed time; the
            // delivery queue retransmits rather than assuming it arrived.
            Err(_elapsed) => Err(TransportError::Timeout {
                endpoint: self.config.endpoint_uri.clone(), // clone: same
                duration: deadline,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint::{
            mqtt::{MqttEndpointConfig, MqttQoS, MqttVersion},
            topic::MqttTopic,
        },
        error::TransportError,
        host_port::HostPort,
        mqtt::{config::MqttPoolConfig, dial::RumqttcDialer, envelope::build_envelope, pool::MqttTransportPool},
        notification_body::NotificationBody,
        notification_context::NotificationContext,
        outcome::DeliveryOutcome,
        oversized_notification::OversizedNotification,
        request::{DeliveryRequest, NotificationDelivery},
        transport::NotificationTransport,
    };
    use andromeda_configuration::{
        address_rule::AddressRule,
        human_byte_size::HumanByteSize,
        mqtt_packet_limit::MqttPacketLimit,
        schema::private_addresses::PrivateAddresses,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_outbound_http::address_policy::AddressPolicy;
    use bytes::BytesMut;
    use rumqttc::{
        ConnAck,
        ConnectReturnCode,
        Packet,
        PubAck,
        QoS,
        SubAck,
        SubscribeReasonCode,
        mqttbytes::Error as PacketError,
        v5::mqttbytes::{
            Error as PacketErrorV5,
            QoS as QoSV5,
            v5::{
                ConnAck as ConnAckV5,
                ConnAckProperties,
                ConnectReturnCode as ConnectReturnCodeV5,
                Packet as PacketV5,
                PingResp as PingRespV5,
                PubAck as PubAckV5,
                SubAck as SubAckV5,
                SubscribeReasonCode as SubscribeReasonCodeV5,
            },
        },
    };
    use serde_json::json;
    use std::{
        future::Future,
        net::SocketAddr,
        str::FromStr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };
    use url::Url;

    const MIB: usize = 1024 * 1024;

    /// The packet `answering` writes back for `packet`, if any.
    fn answer(packet: &Packet, received: &AtomicUsize) -> Option<Packet> {
        match packet {
            Packet::Connect(_) => Some(Packet::ConnAck(ConnAck::new(ConnectReturnCode::Success, false))),
            Packet::Subscribe(subscribe) => Some(Packet::SubAck(SubAck::new(
                subscribe.pkid,
                subscribe.filters.iter().map(|_| SubscribeReasonCode::Success(QoS::AtMostOnce)).collect(),
            ))),
            Packet::Publish(publish) => {
                received.store(publish.payload.len(), Ordering::SeqCst);
                (publish.qos == QoS::AtLeastOnce).then(|| Packet::PubAck(PubAck::new(publish.pkid)))
            }
            Packet::PingReq => Some(Packet::PingResp),
            Packet::ConnAck(_)
            | Packet::PubAck(_)
            | Packet::PubRec(_)
            | Packet::PubRel(_)
            | Packet::PubComp(_)
            | Packet::SubAck(_)
            | Packet::Unsubscribe(_)
            | Packet::UnsubAck(_)
            | Packet::PingResp
            | Packet::Disconnect => None,
        }
    }

    /// Serve one MQTT 3.1.1 session: accept every connect and subscription,
    /// and acknowledge every `QoS` 1 publish, recording its payload length.
    async fn serve(mut stream: TcpStream, received: Arc<AtomicUsize>) {
        let mut buffer = BytesMut::new();
        loop {
            match stream.read_buf(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            loop {
                let packet = match Packet::read(&mut buffer, usize::MAX) {
                    Ok(packet) => packet,
                    Err(PacketError::InsufficientBytes(_)) => break,
                    Err(_) => return,
                };
                if let Some(reply) = answer(&packet, &received) {
                    let mut written = BytesMut::new();
                    reply.write(&mut written, usize::MAX).expect("the reply is written");
                    if stream.write_all(&written).await.is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// A CONNACK accepting the session and announcing `announced` as the
    /// largest packet the server accepts.
    const fn connack_v5(announced: u32) -> ConnAckV5 {
        ConnAckV5 {
            session_present: false,
            code: ConnectReturnCodeV5::Success,
            properties: Some(ConnAckProperties {
                session_expiry_interval: None,
                receive_max: None,
                max_qos: None,
                retain_available: None,
                max_packet_size: Some(announced),
                assigned_client_identifier: None,
                topic_alias_max: None,
                reason_string: None,
                user_properties: Vec::new(),
                wildcard_subscription_available: None,
                subscription_identifiers_available: None,
                shared_subscription_available: None,
                server_keep_alive: None,
                response_information: None,
                server_reference: None,
                authentication_method: None,
                authentication_data: None,
            }),
        }
    }

    /// The packet an MQTT 5 server announcing `announced` writes back for
    /// `packet`, if any.
    fn answer_v5(packet: &PacketV5, received: &AtomicUsize, announced: u32) -> Option<PacketV5> {
        match packet {
            PacketV5::Connect(..) => Some(PacketV5::ConnAck(connack_v5(announced))),
            PacketV5::Subscribe(subscribe) => Some(PacketV5::SubAck(SubAckV5 {
                pkid: subscribe.pkid,
                return_codes: subscribe.filters.iter().map(|_| SubscribeReasonCodeV5::Success(QoSV5::AtMostOnce)).collect(),
                properties: None,
            })),
            PacketV5::Publish(publish) => {
                received.store(publish.payload.len(), Ordering::SeqCst);
                (publish.qos == QoSV5::AtLeastOnce).then(|| PacketV5::PubAck(PubAckV5::new(publish.pkid, None)))
            }
            PacketV5::PingReq(_) => Some(PacketV5::PingResp(PingRespV5)),
            PacketV5::Auth(_)
            | PacketV5::ConnAck(_)
            | PacketV5::PubAck(_)
            | PacketV5::PingResp(_)
            | PacketV5::SubAck(_)
            | PacketV5::PubRec(_)
            | PacketV5::PubRel(_)
            | PacketV5::PubComp(_)
            | PacketV5::Unsubscribe(_)
            | PacketV5::UnsubAck(_)
            | PacketV5::Disconnect(_) => None,
        }
    }

    /// Serve one MQTT 5 session as [`serve`] does a 3.1.1 one, announcing
    /// `announced` as the largest packet the server accepts.
    async fn serve_v5(mut stream: TcpStream, received: Arc<AtomicUsize>, announced: u32) {
        let mut buffer = BytesMut::new();
        loop {
            match stream.read_buf(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            loop {
                let packet = match PacketV5::read(&mut buffer, None) {
                    Ok(packet) => packet,
                    Err(PacketErrorV5::InsufficientBytes(_)) => break,
                    Err(_) => return,
                };
                if let Some(reply) = answer_v5(&packet, &received, announced) {
                    let mut written = BytesMut::new();
                    reply.write(&mut written, None).expect("the reply is written");
                    if stream.write_all(&written).await.is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// A broker on loopback serving each session with `session`, which records
    /// the payload length of the last publish it received.
    async fn listen<S, F>(session: S) -> (SocketAddr, Arc<AtomicUsize>)
    where
        S: Fn(TcpStream, Arc<AtomicUsize>) -> F + Send + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback listener binds");
        let address = listener.local_addr().expect("the listener has an address");
        let received = Arc::new(AtomicUsize::new(0));
        let recorder = Arc::clone(&received);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(session(stream, Arc::clone(&recorder)));
            }
        });
        (address, received)
    }

    /// An MQTT 3.1.1 broker on loopback.
    async fn broker() -> (SocketAddr, Arc<AtomicUsize>) {
        listen(serve).await
    }

    /// An MQTT 5 broker on loopback announcing `announced` as the largest
    /// packet it accepts.
    async fn broker_v5(announced: u32) -> (SocketAddr, Arc<AtomicUsize>) {
        listen(move |stream, received| serve_v5(stream, received, announced)).await
    }

    fn endpoint(address: SocketAddr, qos: MqttQoS) -> MqttEndpointConfig {
        MqttEndpointConfig {
            authority: HostPort::from_str(&address.to_string()).expect("a valid authority"),
            topic: MqttTopic::from_str("notifications").expect("a valid topic"),
            endpoint_uri: Url::parse(&format!("mqtt://{address}/notifications")).expect("a valid endpoint URI"),
            use_tls: false,
            version: MqttVersion::V311,
            qos,
            username: None,
            password: None,
        }
    }

    /// A notification to `endpoint` whose `data` text holds `bytes` characters.
    fn notification(endpoint: &MqttEndpointConfig, bytes: usize) -> DeliveryRequest {
        let subscription = Identifier::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id");
        let context = NotificationContext::core();
        let payload = json!({"id": "urn:ngsi-ld:Notification:1", "type": "Notification", "data": "x".repeat(bytes)});
        NotificationDelivery::builder()
            .endpoint_uri(&endpoint.endpoint_uri)
            .context(&context)
            .subscription_id(&subscription)
            .build()
            .assemble(NotificationBody::written(&payload).expect("a written notification"))
    }

    fn pool(config: MqttPoolConfig) -> Arc<MqttTransportPool> {
        Arc::new(MqttTransportPool::new(config, Arc::new(RumqttcDialer)))
    }

    fn packet_limit_mib(mib: u64) -> MqttPacketLimit {
        MqttPacketLimit::try_from(HumanByteSize::mib(mib)).expect("a limit within the protocol")
    }

    #[tokio::test]
    async fn a_20_mib_notification_is_acknowledged_at_qos_1_under_the_default_packet_limit() {
        let (address, received) = broker().await;
        let endpoint = endpoint(address, MqttQoS::AtLeast);
        let request = notification(&endpoint, 20 * MIB);
        let handle = pool(MqttPoolConfig::default()).transport_for(&endpoint);

        let outcome = handle.deliver(&request).await.expect("the notification is acknowledged");

        assert!(matches!(outcome, DeliveryOutcome::Success { .. }), "{outcome:?}");
        let envelope = build_envelope(&request).expect("the envelope is written");
        pretty_assertions::assert_eq!(received.load(Ordering::SeqCst), envelope.len());
    }

    #[tokio::test]
    async fn a_qos_0_notification_is_delivered_once_the_client_has_written_it() {
        let (address, _received) = broker().await;
        let endpoint = endpoint(address, MqttQoS::AtMost);
        let handle = pool(MqttPoolConfig::default()).transport_for(&endpoint);

        let outcome = handle.deliver(&notification(&endpoint, 64)).await.expect("the notification is written");

        assert!(matches!(outcome, DeliveryOutcome::Success { .. }), "{outcome:?}");
    }

    /// The oversized notification is refused before the client sees it, so
    /// the connection survives and carries the next notification.
    #[tokio::test]
    async fn a_qos_0_notification_over_the_packet_limit_is_refused_without_a_retry_and_the_connection_stays_up() {
        let (address, _received) = broker().await;
        let endpoint = endpoint(address, MqttQoS::AtMost);
        let config = MqttPoolConfig {
            max_packet_size: packet_limit_mib(1),
            ..MqttPoolConfig::default()
        };
        let handle = pool(config).transport_for(&endpoint);

        let refused = handle
            .deliver(&notification(&endpoint, 2 * MIB))
            .await
            .map(drop)
            .expect_err("the notification is too large");
        assert!(
            matches!(
                refused,
                TransportError::Oversized(OversizedNotification::MaxMqttPacket { limit: 1_048_576, .. })
            ),
            "{refused:?}"
        );
        assert!(!refused.is_retryable(), "every attempt would publish the same packet");

        let small = notification(&endpoint, 64);
        let outcome = handle.deliver(&small).await.expect("the connection still delivers");
        assert!(matches!(outcome, DeliveryOutcome::Success { .. }), "{outcome:?}");
    }

    /// The announced maximum is refused before the client sees the packet, so
    /// nothing reaches the server and the connection carries the next
    /// notification.
    #[tokio::test]
    async fn an_mqtt_5_notification_over_the_maximum_the_server_announced_is_refused_naming_the_server_and_max_mqtt_packet() {
        let (address, received) = broker_v5(1_048_576).await;
        let endpoint = MqttEndpointConfig {
            version: MqttVersion::V5,
            ..endpoint(address, MqttQoS::AtLeast)
        };
        let handle = pool(MqttPoolConfig::default()).transport_for(&endpoint);

        let refused = handle
            .deliver(&notification(&endpoint, 2 * MIB))
            .await
            .map(drop)
            .expect_err("the notification exceeds the announced maximum");
        assert!(
            matches!(
                refused,
                TransportError::Oversized(OversizedNotification::MqttServerAnnounced {
                    announced: 1_048_576,
                    configured: 134_217_728,
                    ..
                })
            ),
            "{refused:?}"
        );
        assert!(!refused.is_retryable(), "every attempt would publish the same packet");
        pretty_assertions::assert_eq!(received.load(Ordering::SeqCst), 0, "nothing was published to the server");

        let small = notification(&endpoint, 64);
        let outcome = handle.deliver(&small).await.expect("the connection still delivers");
        assert!(matches!(outcome, DeliveryOutcome::Success { .. }), "{outcome:?}");
        pretty_assertions::assert_eq!(received.load(Ordering::SeqCst), build_envelope(&small).expect("the envelope is written").len());
    }

    #[tokio::test]
    async fn an_mqtt_5_notification_over_max_mqtt_packet_is_refused_naming_it_when_the_server_announced_more() {
        let (address, received) = broker_v5(2 * 1_048_576).await;
        let endpoint = MqttEndpointConfig {
            version: MqttVersion::V5,
            ..endpoint(address, MqttQoS::AtLeast)
        };
        let config = MqttPoolConfig {
            max_packet_size: packet_limit_mib(1),
            ..MqttPoolConfig::default()
        };
        let handle = pool(config).transport_for(&endpoint);

        let refused = handle
            .deliver(&notification(&endpoint, MIB + MIB / 2))
            .await
            .map(drop)
            .expect_err("the notification exceeds max_mqtt_packet");
        assert!(
            matches!(
                refused,
                TransportError::Oversized(OversizedNotification::MaxMqttPacket { limit: 1_048_576, .. })
            ),
            "{refused:?}"
        );
        pretty_assertions::assert_eq!(received.load(Ordering::SeqCst), 0, "nothing was published to the server");
    }

    #[tokio::test]
    async fn a_loopback_broker_is_refused_before_it_is_dialled_when_private_addresses_are_refused() {
        let (address, received) = broker().await;
        let endpoint = endpoint(address, MqttQoS::AtLeast);
        let config = MqttPoolConfig {
            addresses: AddressPolicy::new(PrivateAddresses::Refused, Vec::new(), Vec::new()),
            ..MqttPoolConfig::default()
        };
        let handle = pool(config).transport_for(&endpoint);

        let refused = handle
            .deliver(&notification(&endpoint, 64))
            .await
            .map(drop)
            .expect_err("a loopback broker is refused");

        assert!(matches!(refused, TransportError::PrivateIpBlocked { .. }), "{refused:?}");
        assert!(!refused.is_retryable());
        pretty_assertions::assert_eq!(received.load(Ordering::SeqCst), 0, "nothing was published to the refused broker");
    }

    fn loopback_rule() -> Vec<AddressRule> {
        vec!["127.0.0.0/8".parse().expect("a well-formed rule")]
    }

    #[tokio::test]
    async fn a_loopback_broker_is_published_to_when_its_subnet_is_allowed_though_private_addresses_are_refused() {
        let (address, received) = broker().await;
        let endpoint = endpoint(address, MqttQoS::AtLeast);
        let config = MqttPoolConfig {
            addresses: AddressPolicy::new(PrivateAddresses::Refused, loopback_rule(), Vec::new()),
            ..MqttPoolConfig::default()
        };
        let handle = pool(config).transport_for(&endpoint);

        let request = notification(&endpoint, 64);
        let outcome = handle.deliver(&request).await.expect("an allowed broker is published to");

        assert!(matches!(outcome, DeliveryOutcome::Success { .. }), "{outcome:?}");
        pretty_assertions::assert_eq!(
            received.load(Ordering::SeqCst),
            build_envelope(&request).expect("the envelope is written").len()
        );
    }

    #[tokio::test]
    async fn a_loopback_broker_is_refused_when_its_subnet_is_denied_though_private_addresses_are_allowed() {
        let (address, received) = broker().await;
        let endpoint = endpoint(address, MqttQoS::AtLeast);
        let config = MqttPoolConfig {
            addresses: AddressPolicy::new(PrivateAddresses::Allowed, Vec::new(), loopback_rule()),
            ..MqttPoolConfig::default()
        };
        let handle = pool(config).transport_for(&endpoint);

        let refused = handle
            .deliver(&notification(&endpoint, 64))
            .await
            .map(drop)
            .expect_err("a denied broker is refused");

        assert!(matches!(refused, TransportError::PrivateIpBlocked { .. }), "{refused:?}");
        pretty_assertions::assert_eq!(received.load(Ordering::SeqCst), 0, "nothing was published to the denied broker");
    }
}
