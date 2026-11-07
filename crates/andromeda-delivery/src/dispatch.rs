//! Routing one notification to the transport its endpoint named.

use crate::{
    endpoint::transport::EndpointTransport,
    endpoint_failure::EndpointFailure,
    error::TransportError,
    http::{config::HttpTransportConfig, transport::HttpTransport},
    mqtt::{config::MqttPoolConfig, dial::RumqttcDialer, handle::MqttTransportHandle, pool::MqttTransportPool},
    outcome::DeliveryOutcome,
    request::DeliveryRequest,
    transport::NotificationTransport,
};
use andromeda_telemetry::delivery_ops::count_oversized_notification;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, warn};

/// Unified dispatcher routing to HTTP or MQTT.
pub struct TransportDispatcher {
    /// The HTTP transport, which owns its own client.
    http: HttpTransport,
    /// The MQTT connection pool.
    mqtt_pool: Arc<MqttTransportPool>,
}

/// Result of a notification delivery attempt.
///
/// A failure keeps why it failed, which is also what decides whether it is
/// worth another attempt. Collapsing every failure to one answer would retry
/// a permanently-rejecting `400` through its whole budget exactly like a
/// transient `503`.
pub enum DeliveryResult {
    /// The endpoint accepted the notification.
    Delivered,
    /// The endpoint refused the notification or was not reached.
    Failed(EndpointFailure),
}

impl DeliveryResult {
    /// Whether the endpoint accepted the notification.
    #[must_use]
    pub const fn is_delivered(&self) -> bool {
        matches!(self, DeliveryResult::Delivered)
    }

    /// Whether a failed delivery should be retried: after a `5xx` or `429`
    /// rejection, a timeout, a lost connection, a DNS failure or an
    /// unacknowledged MQTT publish. A `4xx` rejection (other than `429`), a
    /// payload over the limit, a blocked endpoint or a serialisation failure
    /// must not be, or the delivery queue burns its whole attempt budget on a
    /// delivery that can never succeed.
    #[must_use]
    pub const fn should_retry(&self) -> bool {
        match self {
            DeliveryResult::Delivered => false,
            DeliveryResult::Failed(failure) => failure.is_retryable(),
        }
    }
}

/// Turn a transport's answer into the result the workers act on.
fn classify(outcome: Result<DeliveryOutcome, TransportError>) -> DeliveryResult {
    match outcome {
        Ok(DeliveryOutcome::Success { .. }) => DeliveryResult::Delivered,
        Ok(DeliveryOutcome::Rejected { reason, retryable }) => DeliveryResult::Failed(EndpointFailure::Rejected { reason, retryable }),
        Err(error) => DeliveryResult::Failed(EndpointFailure::Transport(error)),
    }
}

impl TransportDispatcher {
    /// Build a dispatcher over the HTTP and MQTT transports.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::ClientInitializationFailed`] when the HTTP
    /// client cannot be built from `http_config`. The MQTT pool is constructed
    /// without a fallible step, so no other variant can arise here.
    pub fn new(http_config: HttpTransportConfig, mqtt_config: MqttPoolConfig) -> Result<TransportDispatcher, TransportError> {
        Ok(TransportDispatcher {
            http: HttpTransport::new(http_config)?,
            mqtt_pool: Arc::new(MqttTransportPool::new(mqtt_config, Arc::new(RumqttcDialer))),
        })
    }

    /// Deliver a notification, logging the outcome and classifying a failure by
    /// whether it is worth retrying.
    ///
    /// The payload limit is left to each transport: the HTTP transport bounds
    /// the notification body, and the MQTT transport the whole packet carrying
    /// its envelope. Either answers `Oversized`, which is not retried and is
    /// counted under the limit that refused it.
    pub async fn deliver(&self, endpoint: &EndpointTransport, request: &DeliveryRequest) -> DeliveryResult {
        let handle = match endpoint {
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
                reason = %reason,
                retryable,
                "notification rejected by endpoint"
            ),
            Err(error) => {
                if let TransportError::Oversized(refusal) = error {
                    count_oversized_notification(refusal.limit());
                }
                warn!(
                    subscription_id = %request.subscription_id,
                    endpoint = %request.endpoint_uri,
                    error = %error,
                    "notification delivery failed"
                );
            }
        }
        classify(outcome)
    }

    /// The MQTT pool, for the health indicator and the probe task.
    #[must_use]
    pub const fn mqtt_pool(&self) -> &Arc<MqttTransportPool> {
        &self.mqtt_pool
    }

    /// Gracefully shut down every transport that holds a persistent connection.
    ///
    /// Only MQTT does; the HTTP client cleans its pool up on drop.
    pub async fn shutdown(&self) {
        self.mqtt_pool.shutdown_all().await;
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
        dispatch::classify,
        error::TransportError,
        outcome::{DeliveryOutcome, EndpointRejection},
        oversized_notification::OversizedNotification,
    };
    use std::{
        net::{IpAddr, Ipv4Addr},
        time::Duration,
    };
    use url::Url;

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
            reason: EndpointRejection::HttpStatus { status: 400 },
            retryable: false,
        }));
        assert!(!result.is_delivered());
        assert!(!result.should_retry());
    }

    #[test]
    fn a_5xx_or_429_rejection_is_retryable() {
        let result = classify(Ok(DeliveryOutcome::Rejected {
            reason: EndpointRejection::HttpStatus { status: 503 },
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

        let too_large = classify(Err(TransportError::Oversized(OversizedNotification::MaxHttpBody { size: 100, limit: 10 })));
        assert!(!too_large.should_retry());
    }
}
