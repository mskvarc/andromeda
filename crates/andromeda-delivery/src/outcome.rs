//! What an endpoint said when it was reached.

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
        reason: String,
        /// Whether attempting the same delivery again could succeed.
        retryable: bool,
    },
}
