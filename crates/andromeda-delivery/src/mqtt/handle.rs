//! Publishing one notification to one MQTT endpoint.

use crate::{
    endpoint::mqtt::{MqttEndpointConfig, MqttQoS},
    error::TransportError,
    mqtt::{
        ack::AckKind,
        connection::MqttClientVariant,
        connection_key::MqttConnectionKey,
        envelope::build_envelope,
        metadata::transported_headers,
        pool::MqttTransportPool,
    },
    outcome::DeliveryOutcome,
    request::DeliveryRequest,
    transport::NotificationTransport,
};
use async_trait::async_trait;
use rumqttc::{
    QoS,
    v5::mqttbytes::{QoS as QoSV5, v5::PublishProperties},
};
use std::{sync::Arc, time::Instant};
use tokio::{sync::oneshot::Receiver, time::timeout};
use tracing::info;

impl MqttQoS {
    /// The MQTT 5 quality-of-service this level is.
    const fn as_v5(self) -> QoSV5 {
        match self {
            MqttQoS::AtMost => QoSV5::AtMostOnce,
            MqttQoS::AtLeast => QoSV5::AtLeastOnce,
            MqttQoS::Exactly => QoSV5::ExactlyOnce,
        }
    }

    /// The MQTT 3.1.1 quality-of-service this level is.
    const fn as_v311(self) -> QoS {
        match self {
            MqttQoS::AtMost => QoS::AtMostOnce,
            MqttQoS::AtLeast => QoS::AtLeastOnce,
            MqttQoS::Exactly => QoS::ExactlyOnce,
        }
    }
}

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
    pub const fn new(pool: Arc<MqttTransportPool>, config: MqttEndpointConfig) -> MqttTransportHandle {
        MqttTransportHandle { pool, config }
    }

    /// The endpoint as a `host:port/topic` label for error messages.
    fn endpoint_label(&self) -> String {
        format!("{}:{}/{}", self.config.host, self.config.port, self.config.topic)
    }

    /// The acknowledgement a publish at this endpoint's QoS is completed by, or
    /// `None` for QoS 0, which is fire-and-forget and never acknowledged.
    const fn completing_ack(&self) -> Option<AckKind> {
        match self.config.qos {
            MqttQoS::AtMost => None,
            MqttQoS::AtLeast => Some(AckKind::PubAck),
            MqttQoS::Exactly => Some(AckKind::PubComp),
        }
    }

    /// A successful delivery outcome, logged.
    fn succeeded(&self, start: Instant) -> DeliveryOutcome {
        info!(
            version = %self.config.version,
            host = %self.config.host,
            port = self.config.port,
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
        let connection = self.pool.get_or_create_connection(&self.config).await?;
        let start = Instant::now();
        let envelope = build_envelope(request);

        // Register the ack waiter and enqueue the publish under one lock, so the
        // event loop assigns packet ids to waiters in the order it sends them.
        let publish_guard = connection.state.publish_lock.lock().await;
        let ack_receiver: Option<Receiver<()>> = self.completing_ack().map(|kind| connection.state.acks().register(kind));

        let published = match &connection.client {
            MqttClientVariant::V5 { client } => {
                let mut properties = PublishProperties {
                    content_type: Some(request.content_type.as_str().to_string()),
                    ..Default::default()
                };
                properties.user_properties = transported_headers(request);

                client
                    .publish_with_properties(&self.config.topic, self.config.qos.as_v5(), false, envelope, properties)
                    .await
                    .map_err(|error| error.to_string())
            }
            // MQTT 3.1.1 has no user properties, so the envelope is the only
            // place the metadata can go.
            MqttClientVariant::V311 { client } => client
                .publish(&self.config.topic, self.config.qos.as_v311(), false, envelope)
                .await
                .map_err(|error| error.to_string()),
        };
        drop(publish_guard);

        if let Err(detail) = published {
            // The packet never reached the wire, so its waiter never will be
            // assigned an id; drop it before it desynchronises the tracker.
            if ack_receiver.is_some() {
                connection.state.acks().cancel_last_registered();
            }
            connection.state.mark_unhealthy();
            return Err(TransportError::ConnectionFailed {
                endpoint: self.endpoint_label(),
                detail,
            });
        }

        // QoS 0 is fire-and-forget: the packet is on its way and there is no
        // acknowledgement to await. Enqueue is deliberately not a health signal
        // — the event loop reports health from real broker traffic — so a queued
        // publish no longer masks a degraded connection.
        let Some(ack_receiver) = ack_receiver else {
            return Ok(self.succeeded(start));
        };

        // QoS 1/2: the delivery is confirmed only once the broker acknowledges
        // it (PUBACK / PUBCOMP). Until then it must not be reported delivered, or
        // an unacked packet the event loop later drops would never retransmit.
        match timeout(self.pool.publish_ack_timeout(), ack_receiver).await {
            Ok(Ok(())) => Ok(self.succeeded(start)),
            // The connection failed before the acknowledgement arrived.
            Ok(Err(_cancelled)) => Err(TransportError::ConnectionFailed {
                endpoint: self.endpoint_label(),
                detail: "connection lost before the broker acknowledged the publish".to_string(),
            }),
            // The broker did not acknowledge within the allowed time; the outbox
            // retransmits rather than assuming it arrived.
            Err(_elapsed) => Err(TransportError::Timeout {
                endpoint: self.endpoint_label(),
                duration: self.pool.publish_ack_timeout(),
            }),
        }
    }

    async fn shutdown(&self) {
        self.pool.disconnect(&MqttConnectionKey::from_config(&self.config)).await;
    }
}
