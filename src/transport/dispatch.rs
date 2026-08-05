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
    sse::{SseConnectionRegistry, SseNotificationEvent},
    types::{DeliveryOutcome, DeliveryRequest, NotificationTransport, TenantHeaderValue, TransportError, content_type_for_accept},
    ws::{WsConnectionRegistry, WsNotificationMessage},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;

/// Unified transport dispatcher routing to HTTP, MQTT, SSE, or WebSocket.
pub struct TransportDispatcher {
    http: HttpTransport,
    mqtt_pool: Arc<MqttTransportPool>,
    sse_registry: Arc<SseConnectionRegistry>,
    ws_registry: Arc<WsConnectionRegistry>,
}

/// Result of a notification delivery attempt.
pub enum DeliveryResult {
    /// Delivery succeeded.
    Delivered,
    /// Delivery failed (transport error or endpoint rejection).
    Failed,
}

impl TransportDispatcher {
    pub fn new(
        http_config: HttpTransportConfig,
        mqtt_config: MqttPoolConfig,
        sse_registry: Arc<SseConnectionRegistry>,
        ws_registry: Arc<WsConnectionRegistry>,
    ) -> Result<TransportDispatcher, TransportError> {
        let http = HttpTransport::new(http_config)?;
        let mqtt_pool = Arc::new(MqttTransportPool::new(mqtt_config));
        Ok(TransportDispatcher {
            http,
            mqtt_pool,
            sse_registry,
            ws_registry,
        })
    }

    /// Return a transport handle for the given endpoint.
    fn transport_for(&self, endpoint: &EndpointTransport) -> TransportHandle<'_> {
        match endpoint {
            EndpointTransport::Http => TransportHandle::Http(&self.http),
            EndpointTransport::Mqtt(config) => TransportHandle::Mqtt(self.mqtt_pool.transport_for(config)),
            EndpointTransport::Sse => unreachable!("SSE delivery bypasses TransportHandle"),
            EndpointTransport::WebSocket => unreachable!("WebSocket delivery bypasses TransportHandle"),
        }
    }

    /// Deliver a notification, handling payload size validation and logging.
    ///
    /// This is the primary entry point for all worker delivery. It resolves
    /// the transport, enforces payload limits, delivers, and logs the
    /// outcome including duration and status details. SSE subscriptions are
    /// delivered via the connection registry rather than the transport trait.
    pub async fn deliver(&self, endpoint: &EndpointTransport, request: &DeliveryRequest) -> DeliveryResult {
        // SSE / WebSocket: route through the connection registries instead of the transport trait.
        if matches!(endpoint, EndpointTransport::Sse) {
            return self.deliver_sse(request);
        }
        if matches!(endpoint, EndpointTransport::WebSocket) {
            return self.deliver_ws(request);
        }

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

    /// Deliver a notification to a connected SSE client via the registry.
    ///
    /// Extracts the notification ID from the JSON payload for use as the SSE
    /// `id:` field (NGSI-LD clause 5.3.1). Falls back to the subscription ID
    /// if the payload cannot be parsed or lacks an `id` field.
    fn deliver_sse(&self, request: &DeliveryRequest) -> DeliveryResult {
        let data = String::from_utf8_lossy(&request.payload).into_owned();
        // Extract notification ID from payload for SSE event identity (clause 5.3.1).
        let event_id = serde_json::from_slice::<Value>(&request.payload)
            .ok()
            .and_then(|v| v.get("id")?.as_str().map(String::from))
            .unwrap_or_else(|| request.subscription_id.clone()); // clone: fallback owned string
        let event = SseNotificationEvent { event_id, data };
        match self.sse_registry.try_send(&request.subscription_id, event) {
            Ok(()) => {
                debug!(
                    subscription_id = %request.subscription_id,
                    "SSE notification delivered"
                );
                DeliveryResult::Delivered
            }
            Err(e) => {
                debug!(
                    subscription_id = %request.subscription_id,
                    error = %e,
                    "SSE notification delivery failed"
                );
                DeliveryResult::Failed
            }
        }
    }

    /// Deliver a notification to a connected WebSocket client via the registry.
    ///
    /// Sends the JSON payload as a text frame. Unlike SSE, there is no
    /// protocol-level event ID -- the notification ID lives in the JSON `id`
    /// field.
    fn deliver_ws(&self, request: &DeliveryRequest) -> DeliveryResult {
        let data = String::from_utf8_lossy(&request.payload).into_owned();
        let message = WsNotificationMessage { data };
        match self.ws_registry.try_send(&request.subscription_id, message) {
            Ok(()) => {
                debug!(
                    subscription_id = %request.subscription_id,
                    "WebSocket notification delivered"
                );
                DeliveryResult::Delivered
            }
            Err(e) => {
                debug!(
                    subscription_id = %request.subscription_id,
                    error = %e,
                    "WebSocket notification delivery failed"
                );
                DeliveryResult::Failed
            }
        }
    }

    /// Access the MQTT pool for health probe spawning.
    pub const fn mqtt_pool(&self) -> &Arc<MqttTransportPool> {
        &self.mqtt_pool
    }

    /// Access the SSE connection registry.
    pub const fn sse_registry(&self) -> &Arc<SseConnectionRegistry> {
        &self.sse_registry
    }

    /// Access the WebSocket connection registry.
    pub const fn ws_registry(&self) -> &Arc<WsConnectionRegistry> {
        &self.ws_registry
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
    tenant: Option<&TenantName>,
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
        tenant: tenant.map(TenantHeaderValue::from),
        receiver_info: extract_key_value_pairs(receiver_info),
        subscription_id: subscription_id.to_string(),
        attempt: 0,
    })
}
