//! What can go wrong between this broker and a notification endpoint.

use std::{net::IpAddr, time::Duration};
use url::Url;

/// Transport-level errors that prevent delivery from completing.
///
/// Every variant is a failure to *reach* the endpoint. An endpoint that
/// answers and refuses is not an error — it is
/// [`crate::outcome::DeliveryOutcome::Rejected`], because the difference
/// decides whether a retry has any chance of working.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The connection could not be established, or was lost mid-delivery.
    #[error("connection to {endpoint} failed: {detail}")]
    ConnectionFailed {
        /// The endpoint that could not be reached.
        endpoint: Url,
        /// What the transport reported.
        detail: String,
    },

    /// The endpoint did not answer within the configured timeout.
    #[error("request to {endpoint} timed out after {duration:?}")]
    Timeout {
        /// The endpoint that did not answer.
        endpoint: Url,
        /// How long the transport waited.
        duration: Duration,
    },

    /// The TLS handshake failed.
    #[error("TLS handshake with {endpoint} failed: {detail}")]
    TlsError {
        /// The endpoint whose certificate or handshake was rejected.
        endpoint: Url,
        /// What the TLS stack reported.
        detail: String,
    },

    /// The serialised notification exceeds the transport's configured limit.
    #[error("payload size {size} exceeds limit {limit}")]
    PayloadTooLarge {
        /// Size of the payload that was refused, in bytes.
        size: usize,
        /// The configured limit, in bytes.
        limit: usize,
    },

    /// The endpoint host resolves into address space the broker will not
    /// deliver to.
    #[error("delivery to {endpoint} blocked: resolves to private/reserved IP {ip}")]
    PrivateIpBlocked {
        /// The endpoint whose host was rejected.
        endpoint: Url,
        /// The address that triggered the guard.
        ip: IpAddr,
    },

    /// The endpoint host could not be resolved at all.
    #[error("failed to resolve host for {endpoint}: {detail}")]
    DnsResolutionFailed {
        /// The endpoint whose host could not be resolved.
        endpoint: Url,
        /// What the resolver reported.
        detail: String,
    },

    /// The notification payload could not be serialised for delivery.
    #[error("failed to serialize the notification for {endpoint}: {source}")]
    PayloadSerialization {
        /// The endpoint the notification was destined for.
        endpoint: Url,
        /// The serialisation failure.
        #[source]
        source: serde_json::Error,
    },

    /// The HTTP client could not be constructed.
    ///
    /// Carries no endpoint: it fails once, at startup, before any endpoint is
    /// known, so there is no URI to name and no placeholder is invented for one.
    #[error("HTTP client initialization failed: {detail}")]
    ClientInitializationFailed {
        /// What the client builder reported.
        detail: String,
    },
}

impl TransportError {
    /// Whether reattempting the delivery could succeed.
    ///
    /// Reaching the endpoint failed transiently for connection, timeout, TLS and
    /// DNS errors, so a later attempt may connect. A payload over the limit, a
    /// blocked endpoint, or a payload that cannot be serialised will fail the
    /// same way on every attempt, so retrying only wastes the budget. A client
    /// that could not be built is a startup misconfiguration, not a per-delivery
    /// condition, so it is likewise not retryable.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            TransportError::ConnectionFailed { .. }
            | TransportError::Timeout { .. }
            | TransportError::TlsError { .. }
            | TransportError::DnsResolutionFailed { .. } => true,
            TransportError::PayloadTooLarge { .. }
            | TransportError::PrivateIpBlocked { .. }
            | TransportError::PayloadSerialization { .. }
            | TransportError::ClientInitializationFailed { .. } => false,
        }
    }
}
