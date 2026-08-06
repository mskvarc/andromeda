//! Publishing one notification to one MQTT endpoint.

use crate::{
    endpoint::mqtt::{MqttEndpointConfig, MqttQoS},
    error::TransportError,
    mqtt::{
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
}

#[async_trait]
impl NotificationTransport for MqttTransportHandle {
    async fn deliver(&self, request: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        let connection = self.pool.get_or_create_connection(&self.config).await?;
        let start = Instant::now();
        let envelope = build_envelope(request);

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

        if let Err(detail) = published {
            connection.state.mark_unhealthy();
            return Err(TransportError::ConnectionFailed {
                endpoint: self.endpoint_label(),
                detail,
            });
        }

        connection.state.mark_healthy();

        info!(
            version = %self.config.version,
            host = %self.config.host,
            port = self.config.port,
            topic = %self.config.topic,
            "MQTT publish succeeded"
        );

        Ok(DeliveryOutcome::Success {
            status_code: None,
            duration: start.elapsed(),
        })
    }

    async fn shutdown(&self) {
        self.pool.disconnect(&MqttConnectionKey::from_config(&self.config)).await;
    }
}
