//! Transport dispatcher and delivery request construction.
//!
//! [`TransportDispatcher`] is the single entry point for notification delivery,
//! routing to HTTP or MQTT based on the endpoint transport configuration.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;
use tracing::{debug, warn};

use crate::transport::{
    endpoint::{EndpointTransport, extract_key_value_pairs},
    http::{HttpTransport, HttpTransportConfig},
    mqtt::{MqttPoolConfig, MqttTransportHandle, MqttTransportPool},
    types::{DeliveryOutcome, DeliveryRequest, NotificationTransport, TransportError, content_type_for_accept},
};

/// Unified transport dispatcher routing to HTTP or MQTT.
pub struct TransportDispatcher {
    http: HttpTransport,
    mqtt_pool: Arc<MqttTransportPool>,
}

/// Result of a notification delivery attempt.
pub enum DeliveryResult {
    /// Delivery succeeded.
    Delivered,
    /// Delivery failed (transport error or endpoint rejection).
    Failed,
}

impl TransportDispatcher {
    pub fn new(http_config: HttpTransportConfig, mqtt_config: MqttPoolConfig) -> Result<TransportDispatcher, TransportError> {
        let http = HttpTransport::new(http_config)?;
        let mqtt_pool = Arc::new(MqttTransportPool::new(mqtt_config));
        Ok(TransportDispatcher { http, mqtt_pool })
    }

    /// Return a transport handle for the given endpoint.
    fn transport_for(&self, endpoint: &EndpointTransport) -> TransportHandle<'_> {
        match endpoint {
            EndpointTransport::Http => TransportHandle::Http(&self.http),
            EndpointTransport::Mqtt(config) => TransportHandle::Mqtt(self.mqtt_pool.transport_for(config)),
        }
    }

    /// Deliver a notification, handling payload size validation and logging.
    ///
    /// This is the primary entry point for all worker delivery. It resolves
    /// the transport, enforces payload limits, delivers, and logs the
    /// outcome including duration and status details.
    pub async fn deliver(&self, endpoint: &EndpointTransport, request: &DeliveryRequest) -> DeliveryResult {
        let max_payload = self.http.max_payload_size();
        if request.payload.len() > max_payload {
            warn!(
                subscription_id = %request.subscription_id,
                size = request.payload.len(),
                limit = max_payload,
                "notification payload exceeds size limit, skipping delivery"
            );
            return DeliveryResult::Failed;
        }

        let handle = self.transport_for(endpoint);

        match handle.deliver(request).await {
            Ok(DeliveryOutcome::Success { status_code, duration }) => {
                debug!(
                    subscription_id = %request.subscription_id,
                    endpoint = %request.endpoint_uri,
                    attempt = request.attempt,
                    ?status_code,
                    ?duration,
                    "notification delivered"
                );
                DeliveryResult::Delivered
            }
            Ok(DeliveryOutcome::Rejected { reason, retryable }) => {
                warn!(
                    subscription_id = %request.subscription_id,
                    endpoint = %request.endpoint_uri,
                    attempt = request.attempt,
                    reason,
                    retryable,
                    "notification rejected by endpoint"
                );
                DeliveryResult::Failed
            }
            Err(e) => {
                warn!(
                    subscription_id = %request.subscription_id,
                    endpoint = %request.endpoint_uri,
                    attempt = request.attempt,
                    error = %e,
                    "notification delivery failed"
                );
                DeliveryResult::Failed
            }
        }
    }

    /// Access the MQTT pool for health probe spawning.
    pub const fn mqtt_pool(&self) -> &Arc<MqttTransportPool> {
        &self.mqtt_pool
    }

    /// Gracefully shut down all transport connections.
    pub async fn shutdown(&self) {
        let handle = TransportHandle::Http(&self.http);
        handle.shutdown().await;
        self.mqtt_pool.shutdown_all().await;
    }
}

/// Enum-dispatched transport handle (avoids boxing).
enum TransportHandle<'a> {
    Http(&'a HttpTransport),
    Mqtt(MqttTransportHandle),
}

#[async_trait]
impl NotificationTransport for TransportHandle<'_> {
    async fn deliver(&self, delivery: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        match self {
            TransportHandle::Http(t) => t.deliver(delivery).await,
            TransportHandle::Mqtt(t) => t.deliver(delivery).await,
        }
    }

    async fn shutdown(&self) {
        match self {
            TransportHandle::Http(t) => t.shutdown().await,
            TransportHandle::Mqtt(t) => t.shutdown().await,
        }
    }
}

/// Build a [`DeliveryRequest`] from worker-level parameters.
///
/// Handles content-type resolution, payload serialization, and receiver_info
/// extraction in a single place.
pub fn build_delivery_request(
    notification_payload: &Value,
    endpoint_uri: &str,
    accept: Option<&str>,
    receiver_info: Option<&[Value]>,
    jsonld_context: Option<&str>,
    tenant: Option<&str>,
    subscription_id: &str,
) -> Result<DeliveryRequest, TransportError> {
    let content_type = content_type_for_accept(accept);
    let payload = serde_json::to_vec(notification_payload).map_err(|e| TransportError::ConnectionFailed {
        endpoint: endpoint_uri.to_string(),
        detail: format!("failed to serialize notification payload: {e}"),
    })?;

    Ok(DeliveryRequest {
        payload: Bytes::from(payload),
        endpoint_uri: endpoint_uri.to_string(),
        content_type,
        context_url: jsonld_context.map(String::from),
        tenant: tenant.map(String::from),
        receiver_info: extract_key_value_pairs(receiver_info),
        subscription_id: subscription_id.to_string(),
        attempt: 0,
    })
}
