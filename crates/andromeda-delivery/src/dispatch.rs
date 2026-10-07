//! Routing one notification to the transport its endpoint named.

use crate::{
    endpoint::transport::EndpointTransport,
    error::TransportError,
    http::{config::HttpTransportConfig, transport::HttpTransport},
    mqtt::{config::MqttPoolConfig, dial::RumqttcDialer, handle::MqttTransportHandle, pool::MqttTransportPool},
    outcome::DeliveryOutcome,
    request::DeliveryRequest,
    stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage},
    transport::NotificationTransport,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;
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
/// Coarser than [`DeliveryOutcome`] (the dispatcher has already logged why)
/// but it keeps the one distinction a retrying caller needs: whether a failure
/// is worth another attempt. Collapsing every failure to one variant would retry
/// a permanently-rejecting `400` through its whole budget exactly like a
/// transient `503`.
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
    #[must_use]
    pub const fn is_delivered(&self) -> bool {
        matches!(self, DeliveryResult::Delivered)
    }

    /// Whether a failed delivery should be retried. A terminal failure must not
    /// be, or the delivery queue burns its whole attempt budget on a delivery that can
    /// never succeed.
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::ClientInitializationFailed`] when the HTTP
    /// client cannot be built from `http_config`. The MQTT pool and both
    /// stream registries are constructed without a fallible step, so no other
    /// variant can arise here.
    pub fn new(
        http_config: HttpTransportConfig,
        mqtt_config: MqttPoolConfig,
        sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
        ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
    ) -> Result<TransportDispatcher, TransportError> {
        Ok(TransportDispatcher {
            http: HttpTransport::new(http_config)?,
            mqtt_pool: Arc::new(MqttTransportPool::new(mqtt_config, Arc::new(RumqttcDialer))),
            sse_registry,
            ws_registry,
        })
    }

    /// Deliver a notification, logging the outcome and classifying a failure by
    /// whether it is worth retrying.
    ///
    /// The payload limit is left to each transport: the HTTP transport enforces
    /// its configured maximum (and answers `PayloadTooLarge`), while an MQTT
    /// publish is bounded by the broker's packet size. Applying the HTTP knob to
    /// an MQTT publish would govern one transport by another's.
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
                ?status_code,
                ?duration,
                "notification delivered"
            ),
            Ok(DeliveryOutcome::Rejected { reason, retryable }) => warn!(
                subscription_id = %request.subscription_id,
                endpoint = %request.endpoint_uri,
                reason,
                retryable,
                "notification rejected by endpoint"
            ),
            Err(error) => warn!(
                subscription_id = %request.subscription_id,
                endpoint = %request.endpoint_uri,
                error = %error,
                "notification delivery failed"
            ),
        }
        classify(outcome)
    }

    /// Push a notification to whichever SSE clients are streaming it.
    ///
    /// The SSE `id:` field is the notification's own identifier so a client
    /// reconnecting with `Last-Event-ID` names something meaningful.
    fn deliver_sse(&self, request: &DeliveryRequest) -> DeliveryResult {
        // The event owns its data, and the request keeps the written notification.
        let data = request.body.text().to_owned();
        // clone: the event owns its id, and the request keeps the notification's own
        let event_id = request.body.id().clone();
        report_stream(
            &request.subscription_id,
            self.sse_registry.try_send(&request.subscription_id, &SseNotificationEvent { event_id, data }),
        )
    }

    /// Push a notification to whichever WebSocket clients are streaming it.
    fn deliver_ws(&self, request: &DeliveryRequest) -> DeliveryResult {
        // The message owns its data, and the request keeps the written notification.
        let data = request.body.text().to_owned();
        report_stream(
            &request.subscription_id,
            self.ws_registry.try_send(&request.subscription_id, &WsNotificationMessage { data }),
        )
    }

    /// The MQTT pool, for the health indicator and the probe task.
    #[must_use]
    pub const fn mqtt_pool(&self) -> &Arc<MqttTransportPool> {
        &self.mqtt_pool
    }

    /// The Server-Sent Events connection registry.
    #[must_use]
    pub const fn sse_registry(&self) -> &Arc<ConnectionRegistry<SseNotificationEvent>> {
        &self.sse_registry
    }

    /// The WebSocket connection registry.
    #[must_use]
    pub const fn ws_registry(&self) -> &Arc<ConnectionRegistry<WsNotificationMessage>> {
        &self.ws_registry
    }

    /// Gracefully shut down every transport that holds a persistent connection.
    ///
    /// Only MQTT does; the HTTP client cleans its pool up on drop.
    pub async fn shutdown(&self) {
        self.mqtt_pool.shutdown_all().await;
    }
}

/// Log and translate the outcome of a push to a streaming client.
fn report_stream<E: Error>(subscription_id: &Identifier, outcome: Result<(), E>) -> DeliveryResult {
    match outcome {
        Ok(()) => {
            debug!(subscription_id = %subscription_id, "streaming notification delivered");
            DeliveryResult::Delivered
        }
        Err(error) => {
            debug!(subscription_id = %subscription_id, error = %error, "streaming notification delivery failed");
            // A stream push has no delivery queue to retry through and the client must
            // reconnect, so a failed push is terminal, not retryable.
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
}

#[cfg(test)]
mod tests {
    use crate::{
        dispatch::{TransportDispatcher, classify},
        endpoint::transport::EndpointTransport,
        error::TransportError,
        http::config::HttpTransportConfig,
        mqtt::config::MqttPoolConfig,
        notification_body::NotificationBody,
        notification_context::NotificationContext,
        outcome::DeliveryOutcome,
        request::{DeliveryRequest, NotificationDelivery},
        stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::{
        net::{IpAddr, Ipv4Addr},
        str::FromStr,
        sync::Arc,
        time::Duration,
    };
    use url::Url;

    /// The delivery of one written notification to a streaming endpoint of
    /// `subscription`.
    fn streamed(subscription: &Identifier, notification_id: &Identifier) -> DeliveryRequest {
        let endpoint = Url::parse("https://receiver.example.com/stream").expect("a well-formed endpoint URI");
        let context = NotificationContext::core();
        let document = serde_json::json!({
            "id": notification_id.as_str(),
            "type": "Notification",
            "subscriptionId": subscription.as_str(),
            "data": []
        });
        NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .context(&context)
            .subscription_id(subscription)
            .build()
            .assemble(NotificationBody::written(notification_id, &document).expect("a written notification"))
    }

    /// Clause 5.3.1: the notification's own `id` is the SSE event `id:` and
    /// rides inside the data of both streams, which carry the written text
    /// unchanged.
    #[tokio::test]
    async fn sse_and_ws_frames_carry_the_notification_id() {
        let subscription = Identifier::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id");
        let notification_id = Identifier::from_str("urn:ngsi-ld:Notification:abc").expect("a well-formed notification id");
        let sse = Arc::new(ConnectionRegistry::<SseNotificationEvent>::new());
        let ws = Arc::new(ConnectionRegistry::<WsNotificationMessage>::new());
        let dispatcher = TransportDispatcher::new(HttpTransportConfig::default(), MqttPoolConfig::default(), Arc::clone(&sse), Arc::clone(&ws))
            .expect("the dispatcher is built");
        let (_sse_connection, mut sse_events, _sse_token) = sse.register(&subscription, 4);
        let (_ws_connection, mut ws_messages, _ws_token) = ws.register(&subscription, 4);
        let request = streamed(&subscription, &notification_id);

        assert!(dispatcher.deliver(&EndpointTransport::Sse, &request).await.is_delivered());
        assert!(dispatcher.deliver(&EndpointTransport::WebSocket, &request).await.is_delivered());

        let event = sse_events.recv().await.expect("an SSE event");
        pretty_assertions::assert_eq!(event.event_id, notification_id);
        pretty_assertions::assert_eq!(event.data.as_str(), request.body.text());
        let message = ws_messages.recv().await.expect("a WebSocket message");
        pretty_assertions::assert_eq!(message.data.as_str(), request.body.text());
        let named: serde_json::Value = serde_json::from_str(&message.data).expect("the frame is JSON");
        pretty_assertions::assert_eq!(named["id"], serde_json::json!(notification_id.as_str()));
    }

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
        let endpoint = Url::parse("https://receiver.example.com/notify").expect("a valid endpoint URI");
        // An MQTT ack timeout and a slow endpoint both surface as Timeout.
        let timeout = classify(Err(TransportError::Timeout {
            endpoint: endpoint.clone(),
            duration: Duration::from_secs(1),
        }));
        assert!(timeout.should_retry());

        let blocked = classify(Err(TransportError::PrivateIpBlocked {
            endpoint,
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }));
        assert!(!blocked.should_retry());

        let too_large = classify(Err(TransportError::PayloadTooLarge { size: 100, limit: 10 }));
        assert!(!too_large.should_retry());
    }
}
