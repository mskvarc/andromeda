//! What can go wrong between this broker and a notification endpoint.

use std::time::Duration;

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
        endpoint: String,
        /// What the transport reported.
        detail: String,
    },

    /// The endpoint did not answer within the configured timeout.
    #[error("request to {endpoint} timed out after {duration:?}")]
    Timeout {
        /// The endpoint that did not answer.
        endpoint: String,
        /// How long the transport waited.
        duration: Duration,
    },

    /// The TLS handshake failed.
    #[error("TLS handshake with {endpoint} failed: {detail}")]
    TlsError {
        /// The endpoint whose certificate or handshake was rejected.
        endpoint: String,
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
        endpoint: String,
        /// The address that triggered the guard.
        ip: String,
    },

    /// The endpoint host could not be resolved at all.
    #[error("failed to resolve host for {endpoint}: {detail}")]
    DnsResolutionFailed {
        /// The endpoint whose host could not be resolved.
        endpoint: String,
        /// What the resolver reported.
        detail: String,
    },

    /// The notification payload could not be serialised for delivery.
    #[error("failed to serialize the notification for {endpoint}: {source}")]
    PayloadSerialization {
        /// The endpoint the notification was destined for.
        endpoint: String,
        /// The serialisation failure.
        #[source]
        source: serde_json::Error,
    },
}
