//! The events a delivery pass's in-flight deliveries already carry.
//!
//! A pass claims one delivery for each slot that frees, and the deliveries of
//! one event sit next to each other in the queue's claim order, so successive
//! claims of a pass keep asking for an event an earlier claim of the same pass
//! read. A stored event never changes once written, and a delivery that
//! references it keeps it from being reaped, so a claim may hand over the
//! event a sibling delivery already holds instead of reading it again.
//!
//! The pass keeps a weak reference only: an event stays held exactly as long
//! as one of its deliveries is in flight, so the pass never keeps a payload
//! alive that no delivery is still sending.

use crate::notification_event::{NotificationEvent, NotificationEventKey};
use andromeda_context_space::context_target::ContextTarget;
use dashmap::DashMap;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

/// The events one delivery pass holds, by branch and then by key.
///
/// An event key is unique only within its branch's queue, so the branch is
/// part of the address.
#[derive(Debug, Default)]
pub struct HeldEvents(DashMap<ContextTarget, HashMap<NotificationEventKey, Weak<NotificationEvent>>>);

impl HeldEvents {
    /// The event under `key` in `target`'s branch, while a delivery of it is
    /// still in flight; `None` when the pass never read it or every delivery
    /// carrying it has been acknowledged.
    #[must_use]
    pub fn get(&self, target: &ContextTarget, key: NotificationEventKey) -> Option<Arc<NotificationEvent>> {
        self.0.get(target)?.get(&key).and_then(Weak::upgrade)
    }

    /// Remember `event`, read from `target`'s branch, for the rest of the pass.
    pub fn hold(&self, target: &ContextTarget, event: &Arc<NotificationEvent>) {
        let mut branch = match self.0.get_mut(target) {
            Some(branch) => branch,
            // clone: the map owns the branch it files the pass's events under, once per branch.
            None => self.0.entry(target.clone()).or_default(),
        };
        branch.insert(event.key, Arc::downgrade(event));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        held_events::HeldEvents,
        notification_event::{NotificationEvent, NotificationEventKey},
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use chrono::Utc;
    use std::sync::Arc;
    use uuid::Uuid;

    fn event(key: i64) -> Arc<NotificationEvent> {
        Arc::new(
            NotificationEvent::builder()
                .key(NotificationEventKey::from(key))
                .write_id(WriteId::from_uuid(Uuid::nil()))
                .event_ordinal(WriteOrdinal::at(1))
                .change(EntityChange::new(EntityLifecycle::Created, Vec::new()))
                .entity_id("urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"))
                .created_at(Utc::now())
                .build(),
        )
    }

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")))
    }

    #[test]
    fn an_event_held_by_an_in_flight_delivery_is_handed_back_as_the_same_allocation() {
        let held = HeldEvents::default();
        let in_flight = event(3);
        held.hold(&ContextTarget::Live, &in_flight);
        let found = held.get(&ContextTarget::Live, NotificationEventKey::from(3_i64)).expect("the held event");
        assert!(Arc::ptr_eq(&found, &in_flight));
    }

    #[test]
    fn an_event_no_delivery_carries_any_more_is_not_held() {
        let held = HeldEvents::default();
        held.hold(&ContextTarget::Live, &event(3));
        assert!(held.get(&ContextTarget::Live, NotificationEventKey::from(3_i64)).is_none());
    }

    #[test]
    fn an_event_the_pass_never_read_is_not_held() {
        let held = HeldEvents::default();
        let in_flight = event(3);
        held.hold(&ContextTarget::Live, &in_flight);
        assert!(held.get(&ContextTarget::Live, NotificationEventKey::from(4_i64)).is_none());
    }

    /// Each branch numbers its own events, so one key names a different event
    /// in every branch.
    #[test]
    fn an_event_key_held_for_one_branch_is_not_held_for_another() {
        let held = HeldEvents::default();
        let live = event(3);
        held.hold(&ContextTarget::Live, &live);
        assert!(held.get(&snapshot(), NotificationEventKey::from(3_i64)).is_none());
        let branch = event(3);
        held.hold(&snapshot(), &branch);
        let found = held.get(&snapshot(), NotificationEventKey::from(3_i64)).expect("the branch's event");
        assert!(Arc::ptr_eq(&found, &branch));
        let found = held.get(&ContextTarget::Live, NotificationEventKey::from(3_i64)).expect("the live event");
        assert!(Arc::ptr_eq(&found, &live));
    }
}
