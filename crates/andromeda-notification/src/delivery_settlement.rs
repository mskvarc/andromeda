//! What a worker records of one delivery it leased once the attempt ends.
//!
//! A settlement addresses one delivery by its branch and key, since a key is
//! unique only within its branch, and says how the attempt ended. The worker
//! hands the queue its settlements in groups, so each one carries everything
//! its own outcome needs: the bounded diagnostic of a failure and the instant
//! the delivery, or its Subscription's next probe, is due again.

use crate::delivery_entry::DeliveryKey;
use andromeda_context_space::context_target::ContextTarget;
use chrono::{DateTime, Utc};

/// The outcome of one leased delivery, as the queue records it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliverySettlement {
    /// Branch whose queue holds the delivery.
    pub target: ContextTarget,
    /// The delivery settled.
    pub key: DeliveryKey,
    /// How its attempt ended.
    pub verdict: DeliveryVerdict,
}

/// How the attempt at a leased delivery ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryVerdict {
    /// The endpoint accepted the notification, or it no longer needs sending;
    /// the delivery leaves the queue and its Subscription's hold is released.
    Delivered,
    /// The attempt failed and is retried: the delivery is due again at
    /// `next_attempt_at`, and its Subscription is held behind a probe.
    Failed {
        /// The failure diagnostic, already within the bound the queue stores.
        detail: String,
        /// When the delivery is due again.
        next_attempt_at: DateTime<Utc>,
    },
    /// No retry can deliver it: the failure is terminal, the attempt reached
    /// the ceiling, or the row could not be read back. When it was its
    /// Subscription's probe, the hold passes on and the next probe is due at
    /// `next_probe_at`.
    Exhausted {
        /// The failure diagnostic, already within the bound the queue stores.
        detail: String,
        /// When the next probe of the delivery's Subscription is due, if the
        /// delivery was its probe.
        next_probe_at: DateTime<Utc>,
    },
}
