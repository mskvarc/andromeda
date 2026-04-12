//! Shared types and trait for notification transport.
//!
//! Defines the [`NotificationTransport`] trait implemented by HTTP and MQTT
//! transports, along with all request/response/error types used across
//! the delivery pipeline.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use mediatype::MediaTypeBuf;

use crate::{
    http::model::response_format::{APPLICATION_GEO_JSON, APPLICATION_JSON, APPLICATION_LD_JSON},
    tenancy::types::TenantName,
};

/// Resolve the notification content type from the subscription's `accept`
/// field, falling back to `application/json`.
pub fn content_type_for_accept(accept: Option<&str>) -> MediaTypeBuf {
    match accept {
        Some("application/ld+json") => APPLICATION_LD_JSON.into(),
        Some("application/geo+json") => APPLICATION_GEO_JSON.into(),
        _ => APPLICATION_JSON.into(),
    }
}

/// Typed wrapper for the tenant identifier in transport-layer headers.
///
/// Used in [`DeliveryRequest`] and HTTP header insertion to avoid raw strings
/// for the `NGSILD-Tenant` header value.
#[derive(Debug, Clone, derive_more::Display, derive_more::AsRef)]
pub struct TenantHeaderValue(String);

impl TenantHeaderValue {
    /// Return the tenant header value as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&TenantName> for TenantHeaderValue {
    fn from(name: &TenantName) -> TenantHeaderValue {
        TenantHeaderValue(name.as_str().to_string())
    }
}

/// Everything a transport needs to deliver a single notification.
#[derive(Debug, Clone)]
pub struct DeliveryRequest {
    /// Serialized notification payload.
    pub payload: Bytes,
    /// Target endpoint URL.
    pub endpoint_uri: String,
    /// Content-Type for the notification body.
    pub content_type: MediaTypeBuf,
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<String>,
    /// Tenant identifier for the `NGSILD-Tenant` header.
    pub tenant: Option<TenantHeaderValue>,
    /// Additional key-value pairs from receiver_info.
    pub receiver_info: Vec<(String, String)>,
    /// Subscription ID for logging and tracking.
    pub subscription_id: String,
    /// Delivery attempt number.
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

    #[error("delivery to {endpoint} blocked: resolves to private/reserved IP {ip}")]
    PrivateIpBlocked { endpoint: String, ip: String },

    #[error("failed to resolve host for {endpoint}: {detail}")]
    DnsResolutionFailed { endpoint: String, detail: String },
}

/// Abstraction over notification delivery transports (HTTP, MQTT).
#[async_trait]
pub trait NotificationTransport: Send + Sync {
    /// Deliver a notification to the configured endpoint.
    async fn deliver(&self, delivery: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError>;

    /// Gracefully close connections. Called during shutdown.
    async fn shutdown(&self);
}
