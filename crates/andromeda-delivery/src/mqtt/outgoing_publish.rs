//! The PUBLISH packet one notification goes out in, measured before it is
//! handed to the client.
//!
//! `rumqttc` checks a packet against the connection's limit only when it
//! writes it, and a packet it refuses there ends the whole connection. A
//! notification over the limit fails the same way on every attempt, so it is
//! measured first, as the client measures it, and refused without a retry
//! while the connection stays up.

use crate::{
    connection_failure::ConnectionFailure,
    endpoint::{mqtt::MqttQoS, topic::MqttTopic},
    error::TransportError,
    mqtt::{connection::MqttClientVariant, metadata::transported_headers},
    request::DeliveryRequest,
};
use rumqttc::{
    AsyncClient,
    Publish,
    QoS,
    v5::{
        AsyncClient as AsyncClientV5,
        mqttbytes::{
            QoS as QoSV5,
            v5::{Publish as PublishV5, PublishProperties},
        },
    },
};

/// A packet identifier the size of every identifier the event loop assigns:
/// a `QoS` 1 or 2 packet carries two bytes for it, whatever its value.
const MEASURED_PACKET_ID: u16 = 1;

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

    /// The packet identifier a publish at this level is measured with.
    const fn measured_packet_id(self) -> u16 {
        match self {
            MqttQoS::AtMost => 0,
            MqttQoS::AtLeast | MqttQoS::Exactly => MEASURED_PACKET_ID,
        }
    }
}

/// A notification's PUBLISH packet, admitted under its connection's limit and
/// ready to hand to the client that writes it.
pub enum OutgoingPublish<'a> {
    /// An MQTT 5 publish.
    V5 {
        /// The client the packet goes out on.
        client: &'a AsyncClientV5,
        /// The packet, its identifier still to be assigned by the event loop.
        packet: PublishV5,
    },
    /// An MQTT 3.1.1 publish.
    V311 {
        /// The client the packet goes out on.
        client: &'a AsyncClient,
        /// The packet, its identifier still to be assigned by the event loop.
        packet: Publish,
    },
}

impl<'a> OutgoingPublish<'a> {
    /// Build the packet carrying `envelope` to `topic`, and admit it when it
    /// fits the largest packet the connection writes.
    ///
    /// An MQTT 5 packet also carries the notification's content type and its
    /// transported headers as properties. MQTT 3.1.1 has no properties, so the
    /// envelope is the only place that metadata travels.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::PayloadTooLarge`] when the whole packet is
    /// larger than the connection writes.
    pub fn admitted(
        client: &'a MqttClientVariant,
        topic: &MqttTopic,
        qos: MqttQoS,
        request: &DeliveryRequest,
        envelope: Vec<u8>,
    ) -> Result<OutgoingPublish<'a>, TransportError> {
        match client {
            MqttClientVariant::V5 { client, max_packet_size } => {
                let properties = PublishProperties {
                    content_type: Some(request.content_type.as_str().to_string()),
                    user_properties: transported_headers(request),
                    ..Default::default()
                };
                let mut packet = PublishV5::new(topic.as_str(), qos.as_v5(), envelope, Some(properties));
                packet.pkid = qos.measured_packet_id();
                admit(packet.size(), *max_packet_size)?;
                Ok(OutgoingPublish::V5 { client, packet })
            }
            MqttClientVariant::V311 { client, max_packet_size } => {
                let mut packet = Publish::new(topic.as_str(), qos.as_v311(), envelope);
                packet.pkid = qos.measured_packet_id();
                admit(packet.size(), *max_packet_size)?;
                Ok(OutgoingPublish::V311 { client, packet })
            }
        }
    }

    /// Queue the packet for the event loop to write.
    ///
    /// The packet's topic was validated as a publish topic when the endpoint
    /// was read, so the client takes it as it stands.
    ///
    /// # Errors
    ///
    /// Returns the client's refusal to queue the packet, which means its event
    /// loop has stopped.
    pub async fn publish(self, topic: &MqttTopic) -> Result<(), ConnectionFailure> {
        match self {
            OutgoingPublish::V5 { client, packet } => {
                let PublishV5 { qos, payload, properties, .. } = packet;
                // Absent properties and default ones encode alike, as a
                // zero-length property section.
                client
                    .publish_bytes_with_properties(topic.as_str(), qos, false, payload, properties.unwrap_or_default())
                    .await
                    .map_err(|error| ConnectionFailure::MqttV5Publish(Box::new(error)))
            }
            OutgoingPublish::V311 { client, packet } => client
                .publish_bytes(topic.as_str(), packet.qos, false, packet.payload)
                .await
                .map_err(|error| ConnectionFailure::MqttV311Publish(Box::new(error))),
        }
    }
}

/// Admit a packet of `size` bytes on a connection writing at most
/// `max_packet_size`.
const fn admit(size: usize, max_packet_size: u32) -> Result<(), TransportError> {
    let limit = max_packet_size as usize;
    if size > limit {
        Err(TransportError::PayloadTooLarge { size, limit })
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        endpoint::{mqtt::MqttQoS, topic::MqttTopic},
        error::TransportError,
        mqtt::{connection::MqttClientVariant, envelope::build_envelope, outgoing_publish::OutgoingPublish},
        notification_body::NotificationBody,
        notification_context::NotificationContext,
        request::{DeliveryRequest, NotificationDelivery},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::notification_media_type::NotificationMediaType;
    use rumqttc::{
        AsyncClient,
        MqttOptions,
        v5::{AsyncClient as AsyncClientV5, MqttOptions as MqttOptionsV5},
    };
    use serde_json::json;
    use std::str::FromStr;
    use url::Url;

    const MIB: usize = 1024 * 1024;

    fn topic() -> MqttTopic {
        MqttTopic::from_str("notifications").expect("a valid publish topic")
    }

    /// A notification whose `data` text holds `bytes` characters.
    fn notification(bytes: usize) -> DeliveryRequest {
        let endpoint = Url::parse("mqtt://broker:1883/notifications").expect("a well-formed endpoint URI");
        let subscription = Identifier::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id");
        let media = NotificationMediaType::from_str("application/json").expect("a notification media type");
        let context = NotificationContext::core();
        let payload = json!({"id": "urn:ngsi-ld:Notification:1", "type": "Notification", "data": "x".repeat(bytes)});
        NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .accept(Some(media))
            .receiver_info(None)
            .context(&context)
            .tenant(None)
            .subscription_id(&subscription)
            .build()
            .assemble(NotificationBody::written(&payload).expect("a written notification"))
    }

    fn v311(max_packet_size: u32) -> MqttClientVariant {
        let (client, _eventloop) = AsyncClient::new(MqttOptions::new("measured", "127.0.0.1", 1883), 8);
        MqttClientVariant::V311 { client, max_packet_size }
    }

    fn v5(max_packet_size: u32) -> MqttClientVariant {
        let (client, _eventloop) = AsyncClientV5::new(MqttOptionsV5::new("measured", "127.0.0.1", 1883), 8);
        MqttClientVariant::V5 { client, max_packet_size }
    }

    fn admitted(client: &MqttClientVariant, qos: MqttQoS, request: &DeliveryRequest) -> Result<(), TransportError> {
        let envelope = build_envelope(request).expect("the envelope is written");
        OutgoingPublish::admitted(client, &topic(), qos, request, envelope).map(drop)
    }

    #[tokio::test]
    async fn a_20_mib_notification_is_admitted_under_the_default_limit_on_either_version() {
        let request = notification(20 * MIB);
        let limit = 128 * 1024 * 1024;
        for qos in [MqttQoS::AtMost, MqttQoS::AtLeast, MqttQoS::Exactly] {
            assert!(admitted(&v311(limit), qos, &request).is_ok());
            assert!(admitted(&v5(limit), qos, &request).is_ok());
        }
    }

    #[tokio::test]
    async fn a_notification_over_the_packet_limit_is_too_large() {
        let request = notification(2 * MIB);
        let limit = 1024 * 1024;
        for client in [v311(limit), v5(limit)] {
            match admitted(&client, MqttQoS::AtLeast, &request) {
                Err(TransportError::PayloadTooLarge { size, limit: refused_at }) => {
                    assert!(size > 2 * MIB, "the whole packet is measured: {size}");
                    assert_eq!(refused_at, 1024 * 1024);
                }
                other => panic!("expected PayloadTooLarge, got {other:?}"),
            }
        }
    }

    /// A `QoS` 1 packet carries a two-byte packet identifier a `QoS` 0 packet
    /// does not, so a limit set exactly to the `QoS` 0 packet refuses the
    /// `QoS` 1 packet of the same notification.
    #[tokio::test]
    async fn the_packet_identifier_counts_towards_the_limit() {
        let request = notification(64);
        let envelope = build_envelope(&request).expect("the envelope is written");
        // 1 fixed header byte, 2 remaining-length bytes, 2 topic length bytes.
        let qos0_size = 1 + 2 + 2 + "notifications".len() + envelope.len();
        let limit = u32::try_from(qos0_size).expect("a small packet");
        assert!(admitted(&v311(limit), MqttQoS::AtMost, &request).is_ok());
        assert!(matches!(
            admitted(&v311(limit), MqttQoS::AtLeast, &request),
            Err(TransportError::PayloadTooLarge { .. })
        ));
    }
}
