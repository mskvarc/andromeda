//! Notification events and deliveries committed with the entity mutation.
//!
//! The matcher hands the commit one pending notification per subscription it
//! matched, which is the shape a delivery worker consumes but not the shape
//! storage keeps: the entity snapshots in it are the same bytes for every
//! subscriber of the same change. They are folded back here into one event per
//! change, carrying the subscriptions it is due to, so the payload is written
//! once and the queue rows that reference it stay narrow.
//!
//! A change that matched no subscription contributes nothing. Persisting an
//! event no delivery references would be storing a notification payload nobody
//! is waiting for, and reaping it later is work the write can simply not create.

use crate::{
    commit_continuation::CommitContinuation,
    commit_refusal::{context_store, name_lookup},
    consequences::ConsequenceWrite,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_invariant::{CommitInvariant, NameDictionary},
    error::CommitError,
    subscription_key::SubscriptionKey,
    subscription_match::SubscriptionMatch,
    write_ordinal::WriteOrdinal,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_attribute_instance::{
    name_keys::{MentionedNames, NameKeys},
    name_resolution::{distinct, resolve_mentioned},
};
use andromeda_postgresql_context_store::notification_delivery::{event_insert::insert_events_in_tx, rows::NewNotificationEvent};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use sqlx::{Postgres, Transaction};

/// One change, and every subscription the matcher found for it.
#[derive(Debug)]
pub(crate) struct EventGroup<'a> {
    /// The pending notification the event's payload columns are taken from.
    ///
    /// Every member of the group agrees on all of them, which is what made them
    /// one group; the first one seen is as good as any other.
    change: &'a SubscriptionMatch,
    /// The subscriptions the change is due to, in matcher order.
    subscriptions: Vec<SubscriptionKey>,
}

/// Persist the notification events of `groups` and the deliveries they owe
/// under the record `continuation` carries, reporting whether any event was
/// written, each recording the commit `continuation` names as the one that
/// made its change.
///
/// `base` continues the commit's one running ordinal counter: temporal history
/// entries have already drawn from it, and every event group here draws the
/// next ordinals in submission order, so `(write_id, event_ordinal)` never
/// collides with a history entry's `(write_id, write_ordinal)` pair of the same
/// commit — the two live in the same numbering rather than each restarting
/// their own count at zero. The groups draw exactly one ordinal each.
///
/// # Errors
///
/// Returns [`CommitError::Persistence`] when a dictionary cannot be resolved
/// or the statement fails, and [`CommitError::Invariant`] when a write produced
/// more events than an ordinal can number or names an attribute the dictionary
/// did not mint.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    groups: Vec<EventGroup<'_>>,
    continuation: CommitContinuation,
    base: WriteOrdinal,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<ConsequenceWrite, CommitError> {
    if groups.is_empty() {
        return Ok(ConsequenceWrite::Nothing);
    }
    let keys = resolve_mentioned(tx, mentioned_names(&groups), entity_type_names, attribute_names)
        .await
        .map_err(|error| name_lookup(&error))?;
    let events = groups
        .into_iter()
        .enumerate()
        .map(|(position, group)| event(base, position, group, &keys))
        .collect::<Result<Vec<NewNotificationEvent<'_>>, CommitError>>()?;
    insert_events_in_tx(tx, target, &continuation.record(), continuation.event_commit(), &events, &keys)
        .await
        .map_err(|error| context_store(error))?;
    Ok(ConsequenceWrite::Written)
}

/// Fold every landed write's pending notifications into one group per change.
///
/// The order is the order the writes and their notifications were submitted in,
/// which is what makes the ordinal each group is numbered by deterministic: a
/// replay of the same commit reproduces the same grouping and therefore the same
/// numbering.
pub(crate) fn group<'a>(landed: &[&'a AtomicWrite]) -> Vec<EventGroup<'a>> {
    let mut groups: Vec<EventGroup<'a>> = Vec::new();
    for change in landed.iter().flat_map(|write| write.subscription_matches.iter()) {
        match groups.iter_mut().find(|group| group.change.same_change(change)) {
            Some(group) => group.subscriptions.push(change.subscription_key),
            None => groups.push(EventGroup {
                change,
                subscriptions: vec![change.subscription_key],
            }),
        }
    }
    groups
}

/// Every name the grouped changes store as a key: the attribute of each
/// changed instance, and every name the states each event stores hold, which
/// its `entity_state_record`s carry as keys.
///
/// Minted in the transaction that first records them: a key may not exist
/// yet, and the same dictionaries the current-attribute rows mint through are
/// used, so one name never ends up with two keys.
fn mentioned_names(groups: &[EventGroup<'_>]) -> MentionedNames {
    let mut mentioned = MentionedNames::default();
    for group in groups {
        // clone: the dictionary request owns the names it resolves.
        mentioned
            .attributes
            // clone: the dictionary request owns the names it resolves.
            .extend(group.change.change.instances.iter().map(|instance| instance.key.attr.clone()));
        for state in group.change.entity_snapshot.iter().chain(&group.change.previous_snapshot) {
            mentioned.collect(state.attributes());
            mentioned.types.extend(state.types().as_slice().iter().cloned()); // clone: the dictionary request owns the names it resolves.
        }
    }
    distinct(mentioned)
}

/// Turn one group into the event row it is written as.
fn event<'a>(base: WriteOrdinal, position: usize, group: EventGroup<'a>, keys: &NameKeys) -> Result<NewNotificationEvent<'a>, CommitError> {
    let delta = u32::try_from(position).map_err(|error| CommitError::from(CommitInvariant::ConsequenceCount(error)))?;
    let ordinal = base
        .advanced_by(delta)
        .map_err(|error| CommitError::from(CommitInvariant::OrdinalOverflow(error)))?;
    let change_attribute_name_keys = group
        .change
        .change
        .instances
        .iter()
        .map(|instance| {
            keys.attribute(&instance.key.attr)
                .ok_or(CommitError::Invariant(CommitInvariant::UnresolvedName(NameDictionary::AttributeName)))
        })
        .collect::<Result<Vec<AttributeNameKey>, CommitError>>()?;
    Ok(NewNotificationEvent {
        ordinal,
        change: &group.change.change,
        entity_id: &group.change.entity_id,
        change_attribute_name_keys,
        entity_snapshot: group.change.entity_snapshot.as_deref(),
        previous_snapshot: group.change.previous_snapshot.as_deref(),
        sys_attrs_snapshot: group.change.sys_attrs_snapshot.as_ref(),
        subscription_keys: group.subscriptions,
    })
}

#[cfg(test)]
mod tests {
    use crate::consequences::notification_events::{EventGroup, event, group, mentioned_names};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        subscription_key::SubscriptionKey,
        subscription_match::SubscriptionMatch,
        write_ordinal::WriteOrdinal,
    };
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        entity_types::EntityTypes,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::SysAttrsSnapshot,
    };
    use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames};
    use chrono::Utc;
    use serde_json::json;
    use std::{ptr, sync::Arc};

    const SPEED: &str = "https://example.org/speed";

    fn uri(value: &str) -> Identifier {
        value.parse().expect("a valid URI")
    }

    fn speed() -> AttributeName {
        SPEED.parse().expect("a valid attribute name")
    }

    /// The keys a commit resolved for `speed` alone.
    fn speed_key() -> NameKeys {
        NameKeys::new(ResolvedNames::from_pairs([(speed(), AttributeNameKey::from(7))]), ResolvedNames::default())
    }

    /// The `Vehicle` entity every match reports a change of.
    fn vehicle() -> Arc<EntityState> {
        Arc::new(EntityState::new(
            uri("urn:ngsi-ld:Vehicle:1"),
            EntityTypes::one("https://example.org/Vehicle".parse().expect("a valid type name")),
        ))
    }

    /// A change through `lifecycle` that `kind`s the `speed` instance of
    /// `dataset`.
    fn entity_change(lifecycle: EntityLifecycle, kind: AttributeChangeKind, dataset: Option<&str>) -> EntityChange {
        EntityChange::new(
            lifecycle,
            vec![AttributeChange {
                key: AttributeInstanceKey {
                    attr: speed(),
                    dataset_id: dataset.map(uri),
                },
                kind,
            }],
        )
    }

    fn matched(subscription: i64, change: EntityChange) -> SubscriptionMatch {
        SubscriptionMatch::builder()
            .subscription_key(SubscriptionKey::try_from(subscription).expect("a positive subscription key"))
            .entity_id(uri("urn:ngsi-ld:Vehicle:1"))
            .change(change)
            .entity_snapshot(Some(vehicle()))
            .build()
    }

    fn updated(subscription: i64) -> SubscriptionMatch {
        matched(subscription, entity_change(EntityLifecycle::Modified, AttributeChangeKind::Updated, None))
    }

    fn created(subscription: i64) -> SubscriptionMatch {
        matched(subscription, entity_change(EntityLifecycle::Created, AttributeChangeKind::Created, None))
    }

    fn write(changes: Vec<SubscriptionMatch>) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(uri("urn:ngsi-ld:Vehicle:1")))
            .entity(EntityMutation::None)
            .subscription_matches(changes)
            .build()
    }

    #[test]
    fn subscribers_of_one_change_share_a_single_event() {
        let written = write(vec![updated(1), updated(2)]);
        let groups = group(&[&written]);
        pretty_assertions::assert_eq!(groups.len(), 1);
        pretty_assertions::assert_eq!(groups[0].subscriptions.len(), 2);
    }

    /// Two writes of one commit changing the same entity differently are two
    /// changes, each notified once (clause 5.8.6).
    #[test]
    fn two_changes_to_one_entity_stay_two_events() {
        let first = write(vec![created(1)]);
        let second = write(vec![updated(1)]);
        let groups = group(&[&first, &second]);
        pretty_assertions::assert_eq!(groups.len(), 2);
    }

    /// A deletion's system attributes are part of what its event stores, so
    /// two deletions that agree on everything else but them are two events.
    #[test]
    fn changes_differing_only_in_their_system_attributes_are_not_folded_together() {
        let mut second = updated(2);
        second.sys_attrs_snapshot = Some(SysAttrsSnapshot {
            created_at: Utc::now(),
            modified_at: Utc::now(),
            expires_at: None,
        });
        let first = write(vec![updated(1)]);
        let other = write(vec![second]);
        pretty_assertions::assert_eq!(group(&[&first, &other]).len(), 2);
    }

    /// The removed entity's system attributes are handed to the event row as
    /// the match carries them.
    #[test]
    fn an_event_carries_the_system_attributes_of_its_change() {
        let mut matched = updated(1);
        let sys_attrs = SysAttrsSnapshot {
            created_at: Utc::now(),
            modified_at: Utc::now(),
            expires_at: None,
        };
        matched.sys_attrs_snapshot = Some(sys_attrs.clone()); // clone: the test compares the event against the value it handed over
        let group = EventGroup {
            change: &matched,
            subscriptions: vec![matched.subscription_key],
        };
        let dictionary = speed_key();
        let built = event(WriteOrdinal::at(0), 0, group, &dictionary).expect("a valid group builds an event");
        pretty_assertions::assert_eq!(built.sys_attrs_snapshot, Some(&sys_attrs));
    }

    /// The ordinal a group is numbered by is its position, so the grouping has
    /// to be a function of submission order alone.
    #[test]
    fn the_grouping_follows_submission_order() {
        let first = write(vec![updated(1)]);
        let second = write(vec![created(2), updated(3)]);
        let groups = group(&[&first, &second]);
        pretty_assertions::assert_eq!(groups.len(), 2);
        pretty_assertions::assert_eq!(groups[0].change.change.lifecycle, EntityLifecycle::Modified);
        pretty_assertions::assert_eq!(groups[0].subscriptions.len(), 2);
        pretty_assertions::assert_eq!(groups[1].change.change.lifecycle, EntityLifecycle::Created);
    }

    #[test]
    fn a_write_that_matched_nothing_produces_no_event() {
        let written = write(Vec::new());
        assert!(group(&[&written]).is_empty());
    }

    #[test]
    fn changes_differing_only_in_their_previous_snapshot_are_not_folded_together() {
        let mut second = updated(2);
        second.previous_snapshot = Some(vehicle());
        assert!(!updated(1).same_change(&second));
    }

    /// The matcher hands every subscriber of one change the same snapshot
    /// allocations, so the fold sees them as one change and the event writes
    /// the payload it was handed once.
    #[test]
    fn changes_sharing_one_snapshot_allocation_fold_together() {
        let snapshot = vehicle();
        let sharing = |subscription: i64| {
            let mut change = updated(subscription);
            change.entity_snapshot = Some(Arc::clone(&snapshot));
            change.previous_snapshot = Some(Arc::clone(&snapshot));
            change
        };
        let written = write(vec![sharing(1), sharing(2), sharing(3)]);
        let groups = group(&[&written]);
        pretty_assertions::assert_eq!(groups.len(), 1);
        pretty_assertions::assert_eq!(groups[0].subscriptions.len(), 3);
        let dictionary = speed_key();
        let row = event(WriteOrdinal::at(0), 0, groups.into_iter().next().expect("one group"), &dictionary).expect("the group becomes one event");
        assert!(row.entity_snapshot.is_some_and(|written| ptr::eq(written, Arc::as_ptr(&snapshot))));
        assert!(row.previous_snapshot.is_some_and(|written| ptr::eq(written, Arc::as_ptr(&snapshot))));
    }

    #[test]
    fn changes_differing_only_in_dataset_identity_are_not_folded_together() {
        let second = matched(
            2,
            entity_change(EntityLifecycle::Modified, AttributeChangeKind::Updated, Some("urn:dataset:named")),
        );
        assert!(!updated(1).same_change(&second));
    }

    /// The commit's temporal history entries have already drawn from the
    /// running ordinal counter by the time notification events are numbered,
    /// so an event's ordinal is the commit's base plus its position, not its
    /// position alone.
    #[test]
    fn an_events_ordinal_is_the_commits_base_plus_its_position() {
        let matched = updated(1);
        let group = EventGroup {
            change: &matched,
            subscriptions: vec![matched.subscription_key],
        };
        let dictionary = speed_key();
        let built = event(WriteOrdinal::at(4), 2, group, &dictionary).expect("a valid group builds an event");
        pretty_assertions::assert_eq!(built.ordinal, WriteOrdinal::at(6));
        pretty_assertions::assert_eq!(built.change_attribute_name_keys, vec![AttributeNameKey::from(7)]);
    }

    /// Every instance names its attribute's key, repeats included, so the
    /// stored arrays stay position for position with the change.
    #[test]
    fn an_event_keys_every_changed_instance_of_the_change() {
        let change = EntityChange::new(
            EntityLifecycle::Modified,
            vec![
                AttributeChange {
                    key: AttributeInstanceKey {
                        attr: speed(),
                        dataset_id: None,
                    },
                    kind: AttributeChangeKind::Updated,
                },
                AttributeChange {
                    key: AttributeInstanceKey {
                        attr: speed(),
                        dataset_id: Some(uri("urn:dataset:named")),
                    },
                    kind: AttributeChangeKind::Deleted,
                },
            ],
        );
        let matched = matched(1, change);
        let group = EventGroup {
            change: &matched,
            subscriptions: vec![matched.subscription_key],
        };
        let dictionary = speed_key();
        let built = event(WriteOrdinal::at(0), 0, group, &dictionary).expect("a valid group builds an event");
        pretty_assertions::assert_eq!(built.change_attribute_name_keys, vec![AttributeNameKey::from(7), AttributeNameKey::from(7)]);
    }

    #[test]
    fn an_attribute_the_dictionary_did_not_mint_is_an_invariant_breach() {
        let matched = updated(1);
        let group = EventGroup {
            change: &matched,
            subscriptions: vec![matched.subscription_key],
        };
        assert!(event(WriteOrdinal::at(0), 0, group, &NameKeys::default()).is_err());
    }

    /// An event stores its states with every name as a key, so the commit
    /// mints the names of both states with the changed attributes, each once.
    #[test]
    fn the_names_a_snapshot_holds_are_resolved_with_the_changed_attributes() {
        let ex = |term: &str| format!("https://example.org/{term}");
        let ngsi = |term: &str| format!("https://uri.etsi.org/ngsi-ld/{term}");
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": [ex("Vehicle")],
            ex("speed"): [{
                "@type": [ngsi("Property")],
                ngsi("hasValue"): [{"@value": 50}],
                ex("accuracy"): [{"@type": [ngsi("Property")], ngsi("hasValue"): [{"@value": 0.5}]}]
            }],
            ex("driver"): [{
                "@type": [ngsi("Relationship")],
                ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Person:1"}],
                ngsi("hasObjectType"): [{"@id": ex("Person")}]
            }]
        }]);
        let current = Arc::new(EntityState::from_slice(document.to_string().as_bytes()).expect("a stored entity"));
        let mut changed = updated(1);
        changed.entity_snapshot = Some(Arc::clone(&current));
        changed.previous_snapshot = Some(current);
        let other = updated(2);
        let written = write(vec![changed, other]);
        let mentioned = mentioned_names(&group(&[&written]));
        let names = |names: &[&str]| names.iter().map(|name| ex(name)).collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            mentioned.attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            names(&["accuracy", "driver", "speed"])
        );
        pretty_assertions::assert_eq!(
            mentioned.types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>(),
            names(&["Person", "Vehicle"])
        );
    }
}
