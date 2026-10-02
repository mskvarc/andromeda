//! One subscription a committing context write owes a notification.

use crate::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::SysAttrsSnapshot};
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// Everything a notification event needs about one subscription that matched.
#[derive(Debug, TypedBuilder)]
pub struct SubscriptionMatch {
    /// Subscription that matched the entity change, by the surrogate key the
    /// notification queue references.
    ///
    /// The key rather than the URI: the queue row this match becomes carries the
    /// key, so handing it over resolved lets the delivery insert write every
    /// matched subscription of a write in one statement without a lookup.
    pub subscription_key: SubscriptionKey,
    /// Entity that triggered the notification.
    pub entity_id: Identifier,
    /// What the write did to the Entity, which the notification reports.
    pub change: EntityChange,
    /// The entity state after the mutation, or the state a deletion removed,
    /// shared by every match of the same change.
    #[builder(default = Default::default())]
    pub entity_snapshot: Option<Arc<EntityState>>,
    /// The entity state before the mutation, shared by every match of the
    /// same change.
    #[builder(default = Default::default())]
    pub previous_snapshot: Option<Arc<EntityState>>,
    /// The clause 4.8 system attributes of an entity a deletion removed.
    ///
    /// By delivery time the entity row is gone and its `createdAt` and
    /// `modifiedAt` can no longer be read back (clause 5.8.6), and an entity
    /// state holds no entity-level system attribute of its own, so they travel
    /// beside the removed state.
    #[builder(default = Default::default())]
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
}

impl SubscriptionMatch {
    /// Whether the two matches describe the same change: everything a stored
    /// notification event holds agrees, not just the entity.
    ///
    /// Two writes of one commit can each change the same entity, and folding
    /// those together would tell a subscriber about one change where clause
    /// 5.8.6 owes it two. The snapshots of one change are shared, so they are
    /// compared by identity first and by content only when they are distinct
    /// allocations.
    #[must_use]
    pub fn same_change(&self, other: &SubscriptionMatch) -> bool {
        self.entity_id == other.entity_id
            && self.change == other.change
            && same_snapshot(self.entity_snapshot.as_ref(), other.entity_snapshot.as_ref())
            && same_snapshot(self.previous_snapshot.as_ref(), other.previous_snapshot.as_ref())
            && self.sys_attrs_snapshot == other.sys_attrs_snapshot
    }
}

/// Whether two optional shared states are the same state.
fn same_snapshot(left: Option<&Arc<EntityState>>, right: Option<&Arc<EntityState>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right) || left == right,
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{subscription_key::SubscriptionKey, subscription_match::SubscriptionMatch};
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use std::sync::Arc;

    fn entity(id: &str) -> EntityState {
        EntityState::new(
            id.parse().expect("valid entity URI"),
            EntityTypes::one("https://example.org/Building".parse().expect("valid type name")),
        )
    }

    fn matched(snapshot: Option<Arc<EntityState>>) -> SubscriptionMatch {
        SubscriptionMatch::builder()
            .subscription_key(SubscriptionKey::try_from(1).expect("a positive key"))
            .entity_id("urn:ngsi-ld:A".parse().expect("valid entity URI"))
            .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
            .entity_snapshot(snapshot)
            .build()
    }

    #[test]
    fn the_builder_defaults_optional_notification_data() {
        let entry = matched(None);
        assert!(entry.entity_snapshot.is_none());
        assert!(entry.previous_snapshot.is_none());
        assert!(entry.sys_attrs_snapshot.is_none());
    }

    /// Matches sharing one snapshot are one change; equal snapshots in
    /// distinct allocations are too, and different ones are not.
    #[test]
    fn matches_are_the_same_change_when_their_snapshots_agree() {
        let shared = Arc::new(entity("urn:ngsi-ld:A"));
        assert!(matched(Some(Arc::clone(&shared))).same_change(&matched(Some(Arc::clone(&shared)))));
        assert!(matched(Some(shared)).same_change(&matched(Some(Arc::new(entity("urn:ngsi-ld:A"))))));
        assert!(!matched(Some(Arc::new(entity("urn:ngsi-ld:A")))).same_change(&matched(Some(Arc::new(entity("urn:ngsi-ld:B"))))));
        assert!(!matched(None).same_change(&matched(Some(Arc::new(entity("urn:ngsi-ld:A"))))));
    }
}
