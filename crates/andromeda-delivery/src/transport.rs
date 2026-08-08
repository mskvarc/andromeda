//! The one thing every delivery protocol has to be able to do.

use crate::{error::TransportError, outcome::DeliveryOutcome, request::DeliveryRequest};
use async_trait::async_trait;

/// Abstraction over notification delivery transports.
///
/// Implemented by the HTTP and MQTT transports and by the dispatcher's
/// internal handle. The streaming transports do not implement it: SSE and
/// WebSocket deliver by pushing into a connection registry rather than by
/// opening one, so they have no outcome beyond "a client was listening".
///
/// Graceful shutdown is not part of this abstraction: the only transport that
/// holds persistent connections is MQTT, whose pool is closed directly through
/// [`MqttTransportPool::shutdown_all`](crate::mqtt::pool::MqttTransportPool::shutdown_all),
/// and the HTTP client cleans its pool up on drop.
///
/// Spelled with `async_trait` because the dispatcher holds transports behind a
/// borrowed handle whose variants have different lifetimes, which a native
/// `async fn` in a trait cannot express without naming each future.
#[async_trait]
pub trait NotificationTransport: Send + Sync {
    /// Deliver a notification to the endpoint the request names.
    #[must_use = "notification delivery must be awaited"]
    async fn deliver(&self, delivery: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError>;
}
