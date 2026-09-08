//! One subscription's pending delivery of a notification event.
//!
//! A delivery is the mutable half of the notification queue: it names the
//! subscription that must be notified, counts the attempts already made and
//! carries the instant the next attempt becomes due. Claiming, acknowledging
//! and retrying all address a delivery, never the event, so two subscriptions
//! interested in the same change retry independently while sharing one stored
//! payload.

use crate::notification_event::NotificationEvent;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use chrono::{DateTime, Utc};
use derive_more::{Display, From};
use typed_builder::TypedBuilder;

/// Unique identifier for a persisted notification delivery.
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeliveryId(i64);

impl DeliveryId {
    /// Returns the raw `i64` value.
    #[must_use]
    pub const fn into_inner(self) -> i64 {
        self.0
    }
}

/// A claimed delivery together with the event payload it must deliver.
#[derive(Clone, Debug, TypedBuilder)]
pub struct DeliveryEntry {
    /// Context branch whose subscription and entity state the event belongs to.
    pub target: ContextTarget,
    /// Auto-generated delivery identifier, the subject of every acknowledgement.
    pub id: DeliveryId,
    /// Subscription that matched the change and must be notified.
    pub subscription_id: Uri,
    /// Number of delivery attempts already made for this subscription.
    pub attempt_count: i32,
    /// Earliest instant at which the next attempt may be made.
    pub next_attempt_at: DateTime<Utc>,
    /// The immutable event this delivery carries.
    pub event: NotificationEvent,
}

#[cfg(test)]
mod tests {
    use crate::delivery_entry::DeliveryId;

    #[test]
    fn a_delivery_id_reports_its_raw_value() {
        assert_eq!(DeliveryId::from(7_i64).into_inner(), 7);
    }

    #[test]
    fn a_delivery_id_displays_as_its_raw_value() {
        assert_eq!(DeliveryId::from(7_i64).to_string(), "7");
    }
}
