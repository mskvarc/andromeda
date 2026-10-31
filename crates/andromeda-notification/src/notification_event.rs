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

use andromeda_context_commit::{store_instant::StoreInstant, write_id::WriteId, write_ordinal::WriteOrdinal};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::SysAttrsSnapshot};
use chrono::{DateTime, Utc};
use derive_more::{Display, From};
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// Database-generated key of a persisted notification event.
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
pub struct NotificationEventKey(i64);

impl NotificationEventKey {
    /// Returns the raw `i64` value.
    #[must_use]
    pub const fn into_inner(self) -> i64 {
        self.0
    }
}

/// One immutable record of an Entity change that at least one subscription matched.
#[derive(Clone, Debug, TypedBuilder)]
pub struct NotificationEvent {
    /// Database-generated event key.
    pub key: NotificationEventKey,
    /// The write that produced this event.
    pub write_id: WriteId,
    /// Deterministic position of this event within its write, unique across the
    /// whole batch so a replayed write cannot produce a second event.
    pub event_ordinal: WriteOrdinal,
    /// What the write did to the Entity: its lifecycle and every Attribute
    /// instance it touched (clause 5.8.6).
    pub change: EntityChange,
    /// Entity whose change produced the event.
    pub entity_id: Identifier,
    /// The entity state the write left behind, or the state a deletion
    /// removed.
    #[builder(default = Default::default())]
    pub entity_snapshot: Option<Arc<EntityState>>,
    /// Entity state before the mutation, carried for `showChanges` support.
    #[builder(default = Default::default())]
    pub previous_snapshot: Option<Arc<EntityState>>,
    /// The clause 4.8 system attributes of the entity as the change left it,
    /// or as a deletion removed it.
    ///
    /// An entity state holds no entity-level `createdAt` or `modifiedAt`, and
    /// by delivery time a later write may have moved the row's `modifiedAt`
    /// on, or a deletion removed the row, so the event records them beside the
    /// state (clause 5.8.6 notifies the entity as the change left it).
    #[builder(default = Default::default())]
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Instant at which the event was recorded.
    pub created_at: DateTime<Utc>,
    /// The store instant of the commit that made the change, which a delivery
    /// compares with the instant its Subscription last became active.
    pub committed_at: StoreInstant,
}

#[cfg(test)]
mod tests {
    use crate::notification_event::NotificationEventKey;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_notification_event_id_reports_its_raw_value() {
        assert_eq!(NotificationEventKey::from(42_i64).into_inner(), 42);
    }

    #[test]
    fn a_notification_event_id_displays_as_its_raw_value() {
        assert_eq!(NotificationEventKey::from(123_i64).to_string(), "123");
    }
}
