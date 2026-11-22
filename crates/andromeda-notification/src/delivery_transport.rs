//! The port a delivery pass sends one claimed notification through, and what
//! it reports back.

use crate::delivery_entry::DeliveryEntry;
use andromeda_delivery::failure_class::FailureClass;
use futures::future::BoxFuture;
use std::time::Duration;

/// Delivery classification that drives notification retry policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// The selected endpoint accepted the notification.
    Delivered,
    /// The subscription no longer exists or no longer matches, so the delivery
    /// is obsolete.
    Obsolete,
    /// The attempt failed.
    Failed {
        /// Whether, and for how long, a later attempt may succeed.
        class: FailureClass,
        /// Diagnostic recorded against the delivery, trimmed before it is stored.
        detail: String,
        /// The wait the endpoint asked for before the next attempt (RFC 9110
        /// section 10.2.3 `Retry-After`), `None` when it asked for none.
        retry_after: Option<Duration>,
    },
    /// A retried delivery was not sent, because its Subscription's clause
    /// 5.2.12 `throttling` window or its endpoint's Table 5.2.15-1 `cooldown`
    /// was still open. It is attempted again once `window` has passed, without
    /// counting an attempt.
    Deferred {
        /// How long until both windows have passed.
        window: Duration,
    },
}

/// Sends one persisted notification without exposing transport implementation
/// to delivery policy.
///
/// Every outcome of one delivery is a [`DeliveryDisposition`], never an error:
/// the transport is the one that knows whether a failure can pass, so it
/// classifies it. A failure a later attempt may overcome, such as an endpoint
/// that did not answer, is [`DeliveryDisposition::Failed`] with a retryable
/// [`FailureClass`]; one the stored delivery can never overcome, such as a
/// payload that cannot be evaluated, is one with [`FailureClass::Permanent`].
/// Without an error channel no single delivery can end the pass its peers were
/// claimed in, nor leave its own lease to run out unacknowledged.
pub trait NotificationDeliveryTransport: Send + Sync + 'static {
    /// Evaluate and send one claimed delivery.
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition>;
}
