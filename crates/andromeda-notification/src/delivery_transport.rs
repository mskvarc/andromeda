//! The port a delivery pass sends one claimed notification through, and what
//! it reports back.

use crate::delivery_entry::DeliveryEntry;
use futures::future::BoxFuture;

/// Delivery classification that drives notification retry policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// The selected endpoint accepted the notification.
    Delivered,
    /// The subscription no longer exists or no longer matches, so the delivery
    /// is obsolete.
    Obsolete,
    /// Delivery may succeed on a later attempt.
    RetryableFailure {
        /// Diagnostic recorded against the delivery, trimmed before it is stored.
        detail: String,
    },
    /// Delivery cannot succeed without changing subscription configuration or
    /// payload.
    TerminalFailure {
        /// Diagnostic recorded against the delivery, trimmed before it is stored.
        detail: String,
    },
}

/// Sends one persisted notification without exposing transport implementation
/// to delivery policy.
///
/// Every outcome of one delivery is a [`DeliveryDisposition`], never an error:
/// the transport is the one that knows whether a failure can pass, so it
/// classifies it. A failure a later attempt may overcome, such as an endpoint
/// that did not answer, is a
/// [`DeliveryDisposition::RetryableFailure`]; one the stored delivery can never
/// overcome, such as a payload that cannot be evaluated, is a
/// [`DeliveryDisposition::TerminalFailure`]. Without an error channel no single
/// delivery can end the pass its peers were claimed in, nor leave its own lease
/// to run out unacknowledged.
pub trait NotificationDeliveryTransport: Send + Sync + 'static {
    /// Evaluate and send one claimed delivery.
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition>;
}
