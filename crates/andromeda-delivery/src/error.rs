//! What can go wrong between this broker and a notification endpoint.

use crate::{connection_failure::ConnectionFailure, http::transport::error_chain_text};
use humantime::format_duration;
use std::{io, net::IpAddr, sync::Arc, time::Duration};
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
    #[error("connection to {endpoint} failed: {cause}")]
    ConnectionFailed {
        /// The endpoint that could not be reached.
        endpoint: Url,
        /// Why the connection failed.
        #[source]
        cause: ConnectionFailure,
    },

    /// The endpoint did not answer within the configured timeout.
    #[error("request to {endpoint} timed out after {}", format_duration(*.duration))]
    Timeout {
        /// The endpoint that did not answer.
        endpoint: Url,
        /// How long the transport waited.
        duration: Duration,
    },

    /// The TLS handshake failed.
    #[error("TLS handshake with {endpoint} failed: {}", error_chain_text(.source))]
    TlsError {
        /// The endpoint whose certificate or handshake was rejected.
        endpoint: Url,
        /// The send failure whose source chain carries the TLS error.
        #[source]
        source: reqwest::Error,
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
    #[error("failed to resolve host for {endpoint}: {source}")]
    DnsResolutionFailed {
        /// The endpoint whose host could not be resolved.
        endpoint: Url,
        /// What the resolver reported, shared with the cached guard outcome.
        #[source]
        source: Arc<io::Error>,
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
    #[error("HTTP client initialization failed: {}", error_chain_text(.0))]
    ClientInitializationFailed(#[source] reqwest::Error),
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
            | TransportError::ClientInitializationFailed(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{connection_failure::ConnectionFailure, error::TransportError};
    use std::{error::Error, io, sync::Arc, time::Duration};
    use url::Url;

    fn endpoint() -> Url {
        Url::parse("http://receiver.example/notify").unwrap_or_else(|error| panic!("fixture endpoint parses: {error}"))
    }

    #[test]
    fn a_timeout_names_its_deadline_in_human_units() {
        let timeout = TransportError::Timeout {
            endpoint: endpoint(),
            duration: Duration::from_millis(1500),
        };
        pretty_assertions::assert_eq!(timeout.to_string(), "request to http://receiver.example/notify timed out after 1s 500ms");
    }

    #[test]
    fn a_connection_failure_names_its_typed_cause() {
        let failed = TransportError::ConnectionFailed {
            endpoint: endpoint(),
            cause: ConnectionFailure::BackoffActive,
        };
        pretty_assertions::assert_eq!(
            failed.to_string(),
            "connection to http://receiver.example/notify failed: connection unhealthy, backoff active"
        );
        assert!(failed.is_retryable());
    }

    #[test]
    fn a_dns_failure_chains_the_resolver_error_as_its_source() {
        let failed = TransportError::DnsResolutionFailed {
            endpoint: endpoint(),
            source: Arc::new(io::Error::new(io::ErrorKind::NotFound, "no such host")),
        };
        pretty_assertions::assert_eq!(failed.to_string(), "failed to resolve host for http://receiver.example/notify: no such host");
        assert!(failed.source().is_some(), "the resolver error is the source");
    }
}
