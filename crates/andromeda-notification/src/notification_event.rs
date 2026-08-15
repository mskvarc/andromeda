//! The immutable notification event a write produces.
//!
//! An event records what changed, once per affected Entity per write, and is
//! never rewritten afterwards: the mutable delivery bookkeeping — attempts,
//! leases, next-attempt instants — lives on the per-subscription delivery that
//! references it. That split is a decision Andromeda made, not a resource ETSI
//! GS CIM 009 defines, which is why the shape lives here rather than in the
//! data model.
//!
//! A write mints one [`WriteId`] and numbers the events it produces with a
//! deterministic [`WriteOrdinal`], so the same write replayed after a crash
//! resolves to the same event rather than a duplicate. A write whose affected
//! Entity matched no subscription produces no event at all.

use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use chrono::{DateTime, Utc};
use derive_more::{Display, From};
use serde_json::Value;
use typed_builder::TypedBuilder;

/// Unique identifier for a persisted notification event.
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
pub struct NotificationEventId(i64);

impl NotificationEventId {
    /// Returns the raw `i64` value.
    pub const fn into_inner(self) -> i64 {
        self.0
    }
}

/// One immutable record of an Entity change that at least one subscription matched.
#[derive(Clone, Debug, TypedBuilder)]
pub struct NotificationEvent {
    /// Auto-generated event identifier.
    pub id: NotificationEventId,
    /// The write that produced this event.
    pub write_id: WriteId,
    /// Deterministic position of this event within its write, unique across the
    /// whole batch so a replayed write cannot produce a second event.
    pub event_ordinal: WriteOrdinal,
    /// Mutation trigger reason (for example `entityCreated`, `entityUpdated`).
    pub trigger: NotificationTrigger,
    /// Entity whose change produced the event.
    pub entity_id: Uri,
    /// Names of the attributes the mutation changed.
    #[builder(default = Default::default())]
    pub changed_attributes: Vec<AttributeName>,
    /// Expanded entity snapshot at the time of the mutation.
    #[builder(default = Default::default())]
    pub entity_snapshot: Option<Value>,
    /// Entity state before the mutation, carried for `showChanges` support.
    #[builder(default = Default::default())]
    pub previous_snapshot: Option<Value>,
    /// Instant at which the event was recorded.
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use crate::notification_event::NotificationEventId;

    #[test]
    fn a_notification_event_id_reports_its_raw_value() {
        assert_eq!(NotificationEventId::from(42_i64).into_inner(), 42);
    }

    #[test]
    fn a_notification_event_id_displays_as_its_raw_value() {
        assert_eq!(NotificationEventId::from(123_i64).to_string(), "123");
    }
}
