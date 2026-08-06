//! The one thing every delivery protocol has to be able to do.

use crate::{error::TransportError, outcome::DeliveryOutcome, request::DeliveryRequest};
use async_trait::async_trait;

/// Abstraction over notification delivery transports.
///
/// Implemented by the HTTP and MQTT transports and by the dispatcher's
/// internal handle. The streaming transports do not implement it: SSE and
/// WebSocket deliver by pushing into a connection registry rather than by
/// opening one, so they have no `shutdown` of their own and no outcome beyond
/// "a client was listening".
///
/// Spelled with `async_trait` because the dispatcher holds transports behind a
/// borrowed handle whose variants have different lifetimes, which a native
/// `async fn` in a trait cannot express without naming each future.
#[async_trait]
pub trait NotificationTransport: Send + Sync {
    /// Deliver a notification to the endpoint the request names.
    async fn deliver(&self, delivery: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError>;

    /// Gracefully close connections. Called during shutdown.
    async fn shutdown(&self);
}
