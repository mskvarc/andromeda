//! Routing one notification to the transport its endpoint named.

use crate::{
    endpoint::transport::EndpointTransport,
    error::TransportError,
    http::{config::HttpTransportConfig, transport::HttpTransport},
    mqtt::{config::MqttPoolConfig, handle::MqttTransportHandle, pool::MqttTransportPool},
    outcome::DeliveryOutcome,
    request::DeliveryRequest,
    stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage},
    transport::NotificationTransport,
};
use async_trait::async_trait;
use serde_json::Value;
use std::{error::Error, sync::Arc};
use tracing::{debug, warn};

/// Unified dispatcher routing to HTTP, MQTT, SSE or WebSocket.
pub struct TransportDispatcher {
    /// The HTTP transport, which owns its own client.
    http: HttpTransport,
    /// The MQTT connection pool.
    mqtt_pool: Arc<MqttTransportPool>,
    /// Clients streaming over Server-Sent Events.
    sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
    /// Clients streaming over WebSocket.
    ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
}

/// Result of a notification delivery attempt.
///
/// Deliberately coarser than [`DeliveryOutcome`]: the dispatcher has already
/// logged why, and every caller does the same two things with the answer.
pub enum DeliveryResult {
    /// Delivery succeeded.
    Delivered,
    /// Delivery failed, whether by transport error or endpoint rejection.
    Failed,
}

impl TransportDispatcher {
    /// Build a dispatcher over the four transports.
    pub fn new(
        http_config: HttpTransportConfig,
        mqtt_config: MqttPoolConfig,
        sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
        ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
    ) -> Result<TransportDispatcher, TransportError> {
        Ok(TransportDispatcher {
            http: HttpTransport::new(http_config)?,
            mqtt_pool: Arc::new(MqttTransportPool::new(mqtt_config)),
            sse_registry,
            ws_registry,
        })
    }

    /// Deliver a notification, enforcing the payload limit and logging the
    /// outcome.
    ///
    /// The streaming transports are handled before the size check on purpose:
    /// the limit exists because an endpoint has to accept a POST body, and a
    /// client that opened its own stream has already accepted whatever comes
    /// down it.
    pub async fn deliver(&self, endpoint: &EndpointTransport, request: &DeliveryRequest) -> DeliveryResult {
        let handle = match endpoint {
            EndpointTransport::Sse => return self.deliver_sse(request),
            EndpointTransport::WebSocket => return self.deliver_ws(request),
            EndpointTransport::Http => TransportHandle::Http(&self.http),
            EndpointTransport::Mqtt(config) => TransportHandle::Mqtt(self.mqtt_pool.transport_for(config)),
        };

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
            Err(error) => {
                warn!(
                    subscription_id = %request.subscription_id,
                    endpoint = %request.endpoint_uri,
                    attempt = request.attempt,
                    error = %error,
                    "notification delivery failed"
                );
                DeliveryResult::Failed
            }
        }
    }

    /// Push a notification to whichever SSE clients are streaming it.
    ///
    /// The SSE `id:` field is the notification's own identifier so a client
    /// reconnecting with `Last-Event-ID` names something meaningful; a payload
    /// without one falls back to the subscription identifier.
    fn deliver_sse(&self, request: &DeliveryRequest) -> DeliveryResult {
        let data = String::from_utf8_lossy(&request.payload).into_owned();
        let event_id = serde_json::from_slice::<Value>(&request.payload)
            .ok()
            .and_then(|payload| payload.get("id")?.as_str().map(String::from))
            .unwrap_or_else(|| request.subscription_id.clone()); // clone: the fallback is owned by the event
        report_stream(
            &request.subscription_id,
            self.sse_registry.try_send(&request.subscription_id, SseNotificationEvent { event_id, data }),
        )
    }

    /// Push a notification to whichever WebSocket clients are streaming it.
    fn deliver_ws(&self, request: &DeliveryRequest) -> DeliveryResult {
        let data = String::from_utf8_lossy(&request.payload).into_owned();
        report_stream(
            &request.subscription_id,
            self.ws_registry.try_send(&request.subscription_id, WsNotificationMessage { data }),
        )
    }

    /// The MQTT pool, for the health indicator and the probe task.
    pub const fn mqtt_pool(&self) -> &Arc<MqttTransportPool> {
        &self.mqtt_pool
    }

    /// The Server-Sent Events connection registry.
    pub const fn sse_registry(&self) -> &Arc<ConnectionRegistry<SseNotificationEvent>> {
        &self.sse_registry
    }

    /// The WebSocket connection registry.
    pub const fn ws_registry(&self) -> &Arc<ConnectionRegistry<WsNotificationMessage>> {
        &self.ws_registry
    }

    /// Gracefully shut down every transport that holds a connection.
    pub async fn shutdown(&self) {
        self.http.shutdown().await;
        self.mqtt_pool.shutdown_all().await;
    }
}

/// Log and translate the outcome of a push to a streaming client.
fn report_stream<E: Error>(subscription_id: &str, outcome: Result<(), E>) -> DeliveryResult {
    match outcome {
        Ok(()) => {
            debug!(subscription_id, "streaming notification delivered");
            DeliveryResult::Delivered
        }
        Err(error) => {
            debug!(subscription_id, error = %error, "streaming notification delivery failed");
            DeliveryResult::Failed
        }
    }
}

/// Enum-dispatched transport handle, which avoids boxing per delivery.
enum TransportHandle<'a> {
    /// The shared HTTP transport.
    Http(&'a HttpTransport),
    /// A handle onto the MQTT pool for one endpoint.
    Mqtt(MqttTransportHandle),
}

#[async_trait]
impl NotificationTransport for TransportHandle<'_> {
    async fn deliver(&self, delivery: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        match self {
            TransportHandle::Http(transport) => transport.deliver(delivery).await,
            TransportHandle::Mqtt(transport) => transport.deliver(delivery).await,
        }
    }

    async fn shutdown(&self) {
        match self {
            TransportHandle::Http(transport) => transport.shutdown().await,
            TransportHandle::Mqtt(transport) => transport.shutdown().await,
        }
    }
}
