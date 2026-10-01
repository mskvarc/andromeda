//! Planning the notifications one mutation owes, on both channels at once.
//!
//! A mutation raises the same change on two channels: the durable
//! [`SubscriptionMatch`] records committed in the mutation's transaction, and
//! the in-process [`ChangeEvent`] stream. Both describe the change the same
//! way, so they are planned together here rather than derived twice and
//! risking divergence.
//!
//! One write's change to one Entity is one [`EntityChange`]: its lifecycle
//! transition and every Attribute instance the write created, updated or
//! deleted. Clause 5.8.6 notifies "the subscribed Entities that changed", and
//! the notification triggers of Table 5.2.12-1 only decide whether a
//! subscription is owed the change, so the change is matched against each
//! subscription once and owes it at most one notification, however many
//! instances it touched.

use crate::{
    change_notifier::ChangeNotifier,
    error::EntityError,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    snapshot_supply::SnapshotSupply,
};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    expanded::{
        attribute_change::{AttributeChangeKind, classify_attribute_changes},
        attribute_expiry::instance_expired,
    },
    notification::ChangeOrigin,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// Both notification channels planned from one mutation.
pub struct NotificationPlan {
    /// Durable records committed in the mutation's transaction, one per matched
    /// subscription.
    pub matches: Vec<SubscriptionMatch>,
    /// In-process change events: the one change the mutation made, or none
    /// when it changed nothing a subscriber can see.
    pub events: Vec<ChangeEvent>,
}

/// The change a write made to an Entity it created, from the payload it stored.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the payload carries an instance whose
/// `datasetId` cannot be read: the request that produced it was admitted.
pub fn creation(current: &Value) -> Result<EntityChange, EntityError> {
    classified(EntityLifecycle::Created, &Value::Null, current)
}

/// The change a write made to an Entity it neither created nor deleted, from
/// the Entity before and after it (clause 5.2.12).
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when either state carries an instance
/// whose `datasetId` cannot be read: the stored state was admitted when it was
/// written, and the new state when its request was.
pub fn modification(previous: &Value, current: &Value) -> Result<EntityChange, EntityError> {
    classified(EntityLifecycle::Modified, previous, current)
}

/// The change a write made to an Entity it deleted, from the Entity it removed.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the removed Entity carries an
/// instance whose `datasetId` cannot be read.
pub(crate) fn deletion(previous: &Value) -> Result<EntityChange, EntityError> {
    classified(EntityLifecycle::Deleted, previous, &Value::Null)
}

/// Classify the instance transitions between two states under `lifecycle`.
fn classified(lifecycle: EntityLifecycle, previous: &Value, current: &Value) -> Result<EntityChange, EntityError> {
    let instances = classify_attribute_changes(previous, current).map_err(|error| EntityError::Internal(error.into()))?;
    Ok(EntityChange::new(lifecycle, instances))
}

/// The change as a subscriber can see it once the instances expired at `at`
/// are treated as absent, or `None` when nothing is left to notify.
///
/// Clause 4.22 makes an instance past its `expiresAt` invalid, so writing one
/// changes nothing a subscriber can see: a created or updated instance that has
/// expired in the Entity the write left behind is dropped from the change. A
/// deleted instance reports what the write removed and is kept as it is -- the
/// removal of an expired instance by the expiry sweep is still a deletion. A
/// modification left with no instance changed nothing at all; a creation or a
/// deletion is still an Entity lifecycle change without one.
fn standing_change(mut change: EntityChange, current: Option<&Value>, at: DateTime<Utc>) -> Option<EntityChange> {
    if let Some(current) = current {
        change.instances.retain(|instance| match instance.kind {
            AttributeChangeKind::Deleted => true,
            AttributeChangeKind::Created | AttributeChangeKind::Updated => {
                !instance_expired(current, instance.key.attr.as_str(), instance.key.dataset_id.as_ref(), at)
            }
        });
    }
    match change.lifecycle {
        EntityLifecycle::Modified if change.instances.is_empty() => None,
        EntityLifecycle::Created | EntityLifecycle::Modified | EntityLifecycle::Deleted => Some(change),
    }
}

impl ChangeNotifier {
    /// Plan both notification channels for one write's `change` to one Entity.
    ///
    /// `entity_snapshot` is the entity as it stands after the mutation -- the
    /// payload a `keyValues`/`normalized` notification renders, never the
    /// request fragment. The previous state is carried for `showChanges` and
    /// for the filters of a change that removed instances.
    #[must_use]
    pub fn plan(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        change: EntityChange,
        entity_snapshot: Option<&Arc<Value>>,
        previous_snapshot: Option<&Arc<Value>>,
    ) -> NotificationPlan {
        let snapshot_ref = entity_snapshot.map(|snapshot| &**snapshot);
        let now = Utc::now();
        let Some(change) = standing_change(change, snapshot_ref, now) else {
            return NotificationPlan {
                matches: Vec::new(),
                events: Vec::new(),
            };
        };
        let mutated = ChangedEntity {
            target,
            id: entity_id,
            types: entity_types,
        };
        let snapshots = MutationSnapshots {
            current: entity_snapshot.map_or(SnapshotSupply::Absent, SnapshotSupply::Ready),
            previous: previous_snapshot,
        };
        let matches = self.subscription_matches(mutated, &change, snapshots);
        let event = ChangeEvent::builder()
            .entity_id(entity_id.clone()) // clone: the event owns its identifier
            .entity_types(entity_types.to_vec())
            .change(change)
            .occurred_at(now)
            .origin(ChangeOrigin::Local)
            .previous_attributes(previous_snapshot.map(Arc::clone))
            .current_payload(entity_snapshot.map(Arc::clone)) // clone: Arc ref-count bump
            .build();
        NotificationPlan { matches, events: vec![event] }
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_plan::{creation, deletion, modification, standing_change};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        expanded::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            instance_key::AttributeInstanceKey,
        },
    };
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("instant parses").with_timezone(&Utc)
    }

    /// An entity whose `speed` expired in 2000 and whose `heading` never expires.
    fn half_expired() -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": "2000-01-01T00:00:00Z"}]
            }],
            "https://example.com/heading": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]
            }]
        }])
    }

    fn instance(attribute: &str, kind: AttributeChangeKind) -> AttributeChange {
        AttributeChange {
            key: AttributeInstanceKey {
                attr: attribute.parse().expect("a valid attribute name"),
                dataset_id: None,
            },
            kind,
        }
    }

    fn property(value: i64) -> Value {
        json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}])
    }

    /// Clause 4.22: writing an instance already past its `expiresAt` changes
    /// nothing a subscriber can see, so it leaves the change, while the other
    /// instance stays.
    #[test]
    fn an_update_to_an_expired_instance_leaves_the_change() {
        let change = EntityChange::new(
            EntityLifecycle::Modified,
            vec![
                instance("https://example.com/speed", AttributeChangeKind::Updated),
                instance("https://example.com/heading", AttributeChangeKind::Updated),
            ],
        );
        let standing = standing_change(change, Some(&half_expired()), instant("2030-01-01T00:00:00Z")).expect("heading still stands");
        pretty_assertions::assert_eq!(standing.instances, vec![instance("https://example.com/heading", AttributeChangeKind::Updated)]);
    }

    /// A modification whose only instance expired changed nothing a subscriber
    /// can see, so no notification is planned at all.
    #[test]
    fn a_modification_left_with_nothing_is_not_notified() {
        let change = EntityChange::new(
            EntityLifecycle::Modified,
            vec![instance("https://example.com/speed", AttributeChangeKind::Updated)],
        );
        assert!(standing_change(change, Some(&half_expired()), instant("2030-01-01T00:00:00Z")).is_none());
    }

    /// The expiry sweep removing an expired instance is still a deletion: a
    /// deletion reports what the write removed.
    #[test]
    fn a_deletion_is_reported_whatever_the_removed_instance_carried() {
        let change = EntityChange::new(
            EntityLifecycle::Modified,
            vec![instance("https://example.com/speed", AttributeChangeKind::Deleted)],
        );
        let standing = standing_change(change, Some(&half_expired()), instant("2030-01-01T00:00:00Z")).expect("the deletion stands");
        pretty_assertions::assert_eq!(standing.instances.len(), 1);
    }

    /// A creation is an Entity lifecycle change even when every instance it
    /// wrote has already expired.
    #[test]
    fn a_creation_stays_a_change_without_standing_instances() {
        let change = EntityChange::new(
            EntityLifecycle::Created,
            vec![instance("https://example.com/speed", AttributeChangeKind::Created)],
        );
        let standing = standing_change(change, Some(&half_expired()), instant("2030-01-01T00:00:00Z")).expect("the creation stands");
        assert_eq!(standing.instances, Vec::<AttributeChange>::new());
    }

    #[test]
    fn a_creation_carries_every_written_instance_as_created() {
        let created = creation(&json!({"https://example.com/a": property(1), "https://example.com/b": property(2)})).expect("readable datasetIds");
        pretty_assertions::assert_eq!(created.lifecycle, EntityLifecycle::Created);
        pretty_assertions::assert_eq!(created.instances.len(), 2);
        assert!(created.instances.iter().all(|instance| instance.kind == AttributeChangeKind::Created));
    }

    #[test]
    fn a_modification_carries_one_precise_transition_per_changed_instance() {
        let before = json!({"https://example.com/keep": property(1), "https://example.com/drop": property(9)});
        let after = json!({"https://example.com/keep": property(2), "https://example.com/new": property(3)});
        let modified = modification(&before, &after).expect("readable datasetIds");
        pretty_assertions::assert_eq!(modified.lifecycle, EntityLifecycle::Modified);
        let kinds: Vec<(&str, AttributeChangeKind)> = modified.instances.iter().map(|instance| (instance.key.attr.as_str(), instance.kind)).collect();
        pretty_assertions::assert_eq!(
            kinds,
            vec![
                ("https://example.com/drop", AttributeChangeKind::Deleted),
                ("https://example.com/keep", AttributeChangeKind::Updated),
                ("https://example.com/new", AttributeChangeKind::Created)
            ]
        );
    }

    #[test]
    fn a_deletion_carries_every_removed_instance_as_deleted() {
        let deleted = deletion(&json!({"https://example.com/a": property(1)})).expect("readable datasetIds");
        pretty_assertions::assert_eq!(deleted.lifecycle, EntityLifecycle::Deleted);
        pretty_assertions::assert_eq!(deleted.instances, vec![instance("https://example.com/a", AttributeChangeKind::Deleted)]);
    }
}
