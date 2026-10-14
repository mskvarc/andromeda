//! What an endpoint said when it was reached.

use crate::error::TransportError;
use andromeda_telemetry::operation::Outcome;
use std::time::Duration;

/// Outcome of a delivery attempt that reached the endpoint.
///
/// Distinct from [`crate::error::TransportError`]: a rejection means the
/// endpoint is there and said no, which is why it carries its own
/// retryability rather than leaving the caller to guess from a message.
#[derive(Debug)]
pub enum DeliveryOutcome {
    /// The endpoint accepted the notification.
    Success {
        /// The protocol status, where the protocol has one. MQTT does not.
        status_code: Option<u16>,
        /// How long the send took. For MQTT this is the local enqueue latency, not a broker round-trip.
        duration: Duration,
    },
    /// The endpoint explicitly rejected the notification.
    Rejected {
        /// What the endpoint said.
        reason: EndpointRejection,
        /// Whether attempting the same delivery again could succeed.
        retryable: bool,
    },
}

/// Why an endpoint that was reached refused a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EndpointRejection {
    /// The endpoint answered an HTTP request with a non-success status.
    #[error("HTTP {status}")]
    HttpStatus {
        /// The response status code.
        status: u16,
    },
}

/// The telemetry [`Outcome`] one delivery attempt represents.
///
/// Only an accepted delivery counts as success; an endpoint rejection and a
/// transport error are both failures, so a bare `Ok`/`Err` split would wrongly
/// count a rejection as a success.
pub(crate) const fn delivery_metric_outcome(result: &Result<DeliveryOutcome, TransportError>) -> Outcome {
    match result {
        Ok(DeliveryOutcome::Success { .. }) => Outcome::Ok,
        Ok(DeliveryOutcome::Rejected { .. }) | Err(_) => Outcome::Error,
    }
}
