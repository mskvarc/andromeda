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
/// Coarser than [`DeliveryOutcome`] — the dispatcher has already logged why —
/// but it keeps the one distinction a retrying caller needs: whether a failure
/// is worth another attempt. Collapsing every failure to one variant is what let
/// a permanently-rejecting `400` be retried through its whole budget exactly like
/// a transient `503`.
pub enum DeliveryResult {
    /// The endpoint accepted the notification.
    Delivered,
    /// Delivery failed but a later attempt could succeed: a `5xx` or `429`
    /// rejection, a timeout, a lost connection, a DNS failure, or an
    /// unacknowledged MQTT publish.
    RetryableFailure,
    /// Delivery failed in a way a retry cannot fix: a `4xx` rejection (other than
    /// `429`), a payload over the limit, a blocked endpoint, or a serialisation
    /// failure.
    TerminalFailure,
}

impl DeliveryResult {
    /// Whether the endpoint accepted the notification.
    pub const fn is_delivered(&self) -> bool {
        matches!(self, DeliveryResult::Delivered)
    }

    /// Whether a failed delivery should be retried. A terminal failure must not
    /// be, or the outbox burns its whole attempt budget on a delivery that can
    /// never succeed.
    pub const fn should_retry(&self) -> bool {
        matches!(self, DeliveryResult::RetryableFailure)
    }
}

/// Classify a transport's answer into the result the workers act on, preserving
/// whether a failure is worth retrying.
fn classify(outcome: Result<DeliveryOutcome, TransportError>) -> DeliveryResult {
    match outcome {
        Ok(DeliveryOutcome::Success { .. }) => DeliveryResult::Delivered,
        Ok(DeliveryOutcome::Rejected { retryable: true, .. }) => DeliveryResult::RetryableFailure,
        Ok(DeliveryOutcome::Rejected { retryable: false, .. }) => DeliveryResult::TerminalFailure,
        Err(error) if error.is_retryable() => DeliveryResult::RetryableFailure,
        Err(_terminal) => DeliveryResult::TerminalFailure,
    }
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

    /// Deliver a notification, logging the outcome and classifying a failure by
    /// whether it is worth retrying.
    ///
    /// The payload limit is left to each transport: the HTTP transport enforces
    /// its configured maximum (and answers `PayloadTooLarge`), while an MQTT
    /// publish is bounded by the broker's packet size. The dispatcher applying
    /// the HTTP knob to an MQTT publish governed one transport by another's.
    pub async fn deliver(&self, endpoint: &EndpointTransport, request: &DeliveryRequest) -> DeliveryResult {
        let handle = match endpoint {
            EndpointTransport::Sse => return self.deliver_sse(request),
            EndpointTransport::WebSocket => return self.deliver_ws(request),
            EndpointTransport::Http => TransportHandle::Http(&self.http),
            EndpointTransport::Mqtt(config) => TransportHandle::Mqtt(self.mqtt_pool.transport_for(config)),
        };

        let outcome = handle.deliver(request).await;
        match &outcome {
            Ok(DeliveryOutcome::Success { status_code, duration }) => debug!(
                subscription_id = %request.subscription_id,
                endpoint = %request.endpoint_uri,
                attempt = request.attempt,
                ?status_code,
                ?duration,
                "notification delivered"
            ),
            Ok(DeliveryOutcome::Rejected { reason, retryable }) => warn!(
                subscription_id = %request.subscription_id,
                endpoint = %request.endpoint_uri,
                attempt = request.attempt,
                reason,
                retryable,
                "notification rejected by endpoint"
            ),
            Err(error) => warn!(
                subscription_id = %request.subscription_id,
                endpoint = %request.endpoint_uri,
                attempt = request.attempt,
                error = %error,
                "notification delivery failed"
            ),
        }
        classify(outcome)
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
            .unwrap_or_else(|| request.subscription_id.to_string()); // the fallback is owned by the event
        report_stream(
            request.subscription_id.as_ref(),
            self.sse_registry
                .try_send(request.subscription_id.as_ref(), SseNotificationEvent { event_id, data }),
        )
    }

    /// Push a notification to whichever WebSocket clients are streaming it.
    fn deliver_ws(&self, request: &DeliveryRequest) -> DeliveryResult {
        let data = String::from_utf8_lossy(&request.payload).into_owned();
        report_stream(
            request.subscription_id.as_ref(),
            self.ws_registry.try_send(request.subscription_id.as_ref(), WsNotificationMessage { data }),
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
            // A stream push has no outbox to retry through — the client must
            // reconnect — so a failed push is terminal, not retryable.
            DeliveryResult::TerminalFailure
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

#[cfg(test)]
mod tests {
    use crate::{dispatch::classify, error::TransportError, outcome::DeliveryOutcome};
    use std::time::Duration;

    #[test]
    fn a_success_is_delivered_and_not_retried() {
        let result = classify(Ok(DeliveryOutcome::Success {
            status_code: Some(204),
            duration: Duration::from_millis(1),
        }));
        assert!(result.is_delivered());
        assert!(!result.should_retry());
    }

    #[test]
    fn a_4xx_rejection_is_terminal_so_it_is_attempted_once_not_through_the_whole_budget() {
        let result = classify(Ok(DeliveryOutcome::Rejected {
            reason: "HTTP 400".to_string(),
            retryable: false,
        }));
        assert!(!result.is_delivered());
        assert!(!result.should_retry());
    }

    #[test]
    fn a_5xx_or_429_rejection_is_retryable() {
        let result = classify(Ok(DeliveryOutcome::Rejected {
            reason: "HTTP 503".to_string(),
            retryable: true,
        }));
        assert!(result.should_retry());
    }

    #[test]
    fn a_timeout_is_retryable_but_a_blocked_endpoint_and_an_oversized_payload_are_terminal() {
        // An MQTT ack timeout and a slow endpoint both surface as Timeout.
        let timeout = classify(Err(TransportError::Timeout {
            endpoint: "e".to_string(),
            duration: Duration::from_secs(1),
        }));
        assert!(timeout.should_retry());

        let blocked = classify(Err(TransportError::PrivateIpBlocked {
            endpoint: "e".to_string(),
            ip: "127.0.0.1".to_string(),
        }));
        assert!(!blocked.should_retry());

        let too_large = classify(Err(TransportError::PayloadTooLarge { size: 100, limit: 10 }));
        assert!(!too_large.should_retry());
    }
}
