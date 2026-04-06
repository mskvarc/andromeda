//! Shared types and trait for notification transport.
//!
//! Defines the [`NotificationTransport`] trait implemented by HTTP and MQTT
//! transports, along with all request/response/error types used across
//! the delivery pipeline.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use mediatype::MediaTypeBuf;

use crate::http::model::response_format::{APPLICATION_GEO_JSON, APPLICATION_JSON, APPLICATION_LD_JSON};

/// Resolve the notification content type from the subscription's `accept`
/// field, falling back to `application/json`.
pub fn content_type_for_accept(accept: Option<&str>) -> MediaTypeBuf {
    match accept {
        Some("application/ld+json") => APPLICATION_LD_JSON.into(),
        Some("application/geo+json") => APPLICATION_GEO_JSON.into(),
        _ => APPLICATION_JSON.into(),
    }
}

/// Everything a transport needs to deliver a single notification.
#[derive(Debug, Clone)]
pub struct DeliveryRequest {
    pub payload: Bytes,
    pub endpoint_uri: String,
    pub content_type: MediaTypeBuf,
    pub context_url: Option<String>,
    pub tenant: Option<String>,
    pub receiver_info: Vec<(String, String)>,
    pub subscription_id: String,
    pub attempt: u32,
}

/// Outcome of a successful transport-level delivery attempt.
#[derive(Debug)]
pub enum DeliveryOutcome {
    /// The endpoint accepted the notification.
    Success { status_code: Option<u16>, duration: Duration },
    /// The endpoint explicitly rejected the notification.
    Rejected { reason: String, retryable: bool },
}

/// Transport-level errors that prevent delivery from completing.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection to {endpoint} failed: {detail}")]
    ConnectionFailed { endpoint: String, detail: String },

    #[error("request to {endpoint} timed out after {duration:?}")]
    Timeout { endpoint: String, duration: Duration },

    #[error("TLS handshake with {endpoint} failed: {detail}")]
    TlsError { endpoint: String, detail: String },

    #[error("payload size {size} exceeds limit {limit}")]
    PayloadTooLarge { size: usize, limit: usize },
}

/// Abstraction over notification delivery transports (HTTP, MQTT).
#[async_trait]
pub trait NotificationTransport: Send + Sync {
    /// Deliver a notification to the configured endpoint.
    async fn deliver(&self, delivery: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError>;

    /// Gracefully close connections. Called during shutdown.
    async fn shutdown(&self);
}
