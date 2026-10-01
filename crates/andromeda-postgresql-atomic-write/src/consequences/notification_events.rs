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
    write_record::WriteRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_context_store::notification_delivery::{event_insert::insert_events_in_tx, rows::NewNotificationEvent};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

/// One change, and every subscription the matcher found for it.
#[derive(Debug)]
struct EventGroup<'a> {
    /// The pending notification the event's payload columns are taken from.
    ///
    /// Every member of the group agrees on all of them, which is what made them
    /// one group; the first one seen is as good as any other.
    change: &'a SubscriptionMatch,
    /// The subscriptions the change is due to, in matcher order.
    subscriptions: Vec<SubscriptionKey>,
}

/// Persist the notification events of landed writes and the deliveries they
/// owe, and report the ordinal one past the last event this phase consumed
/// together with whether any event was written.
///
/// `base` continues the commit's one running ordinal counter: temporal history
/// entries have already drawn from it, and every event group here draws the
/// next ordinals in submission order, so `(write_id, event_ordinal)` never
/// collides with a history entry's `(write_id, write_ordinal)` pair of the same
/// commit — the two live in the same numbering rather than each restarting
/// their own count at zero.
///
/// # Errors
///
/// Returns [`CommitError::Persistence`] when the dictionary cannot be resolved
/// or the statement fails, and [`CommitError::Invariant`] when a write produced
/// more events than an ordinal can number or names an attribute the dictionary
/// did not mint.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    landed: &[&AtomicWrite],
    record: &WriteRecord,
    base: WriteOrdinal,
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<(WriteOrdinal, ConsequenceWrite), CommitError> {
    let groups = group(landed);
    if groups.is_empty() {
        return Ok((base, ConsequenceWrite::Nothing));
    }
    let dictionary = resolve_dictionary(tx, &groups, attribute_names).await?;
    let events = groups
        .into_iter()
        .enumerate()
        .map(|(position, group)| event(base, position, group, &dictionary))
        .collect::<Result<Vec<NewNotificationEvent<'_>>, CommitError>>()?;
    insert_events_in_tx(tx, target, record, &events).await.map_err(|error| context_store(error))?;
    let advance = u32::try_from(events.len()).map_err(|error| CommitError::from(CommitInvariant::ConsequenceCount(error)))?;
    let next = base
        .advanced_by(advance)
        .map_err(|error| CommitError::from(CommitInvariant::OrdinalOverflow(error)))?;
    Ok((next, ConsequenceWrite::Written))
}

/// Fold every landed write's pending notifications into one group per change.
///
/// The order is the order the writes and their notifications were submitted in,
/// which is what makes the ordinal each group is numbered by deterministic: a
/// replay of the same commit reproduces the same grouping and therefore the same
/// numbering.
fn group<'a>(landed: &[&'a AtomicWrite]) -> Vec<EventGroup<'a>> {
    let mut groups: Vec<EventGroup<'a>> = Vec::new();
    for change in landed.iter().flat_map(|write| write.subscription_matches.iter()) {
        match groups.iter_mut().find(|group| same_change(group.change, change)) {
            Some(group) => group.subscriptions.push(change.subscription_key),
            None => groups.push(EventGroup {
                change,
                subscriptions: vec![change.subscription_key],
            }),
        }
    }
    groups
}

/// Whether two pending notifications describe the same change.
///
/// Everything an event stores has to agree, not just the entity: two writes of
/// one commit can each change the same entity, and folding those together would
/// tell a subscriber about one change where clause 5.8.6 owes it two.
fn same_change(left: &SubscriptionMatch, right: &SubscriptionMatch) -> bool {
    left.entity_id == right.entity_id
        && left.change == right.change
        && left.entity_snapshot == right.entity_snapshot
        && left.previous_snapshot == right.previous_snapshot
}

/// Mint the dictionary key of every attribute the grouped changes name.
///
/// Creating rather than looking up: the events are written in the transaction
/// that first records the attribute, so a key may not exist yet, and the same
/// dictionary the current-attribute projection mints through is used so one
/// attribute never ends up with two keys. That transaction's own connection
/// carries the resolution, so an event and the names it reports are one write.
async fn resolve_dictionary(
    tx: &mut Transaction<'_, Postgres>,
    groups: &[EventGroup<'_>],
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<HashMap<AttributeName, AttributeNameKey>, CommitError> {
    let mut names: Vec<AttributeName> = groups
        .iter()
        .flat_map(|group| group.change.change.instances.iter().map(|instance| &instance.key.attr))
        .cloned() // clone: the dictionary request owns the names it resolves.
        .collect();
    names.sort_unstable();
    names.dedup();
    let names = names;
    attribute_names.resolve_or_create(tx, &names).await.map_err(|error| name_lookup(&error))
}

/// Turn one group into the event row it is written as.
fn event<'a>(
    base: WriteOrdinal,
    position: usize,
    group: EventGroup<'a>,
    dictionary: &HashMap<AttributeName, AttributeNameKey>,
) -> Result<NewNotificationEvent<'a>, CommitError> {
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
            dictionary
                .get(&instance.key.attr)
                .copied()
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
        subscription_keys: group.subscriptions,
    })
}

#[cfg(test)]
mod tests {
    use crate::consequences::notification_events::{EventGroup, event, group, same_change};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        subscription_key::SubscriptionKey,
        subscription_match::SubscriptionMatch,
        write_ordinal::WriteOrdinal,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        expanded::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use serde_json::json;
    use std::{collections::HashMap, ptr, sync::Arc};

    const SPEED: &str = "https://example.org/speed";

    fn uri(value: &str) -> Identifier {
        value.parse().expect("a valid URI")
    }

    fn speed() -> AttributeName {
        SPEED.parse().expect("a valid attribute name")
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
            .entity_snapshot(Some(Arc::new(json!({ "id": "urn:ngsi-ld:Vehicle:1" }))))
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
        second.previous_snapshot = Some(Arc::new(json!({ "id": "urn:ngsi-ld:Vehicle:1" })));
        assert!(!same_change(&updated(1), &second));
    }

    /// The matcher hands every subscriber of one change the same snapshot
    /// allocations, so the fold sees them as one change and the event writes
    /// the payload it was handed once.
    #[test]
    fn changes_sharing_one_snapshot_allocation_fold_together() {
        let snapshot = Arc::new(json!({ "id": "urn:ngsi-ld:Vehicle:1", "speed": [1, 2, 3] }));
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
        let dictionary = HashMap::from([(speed(), AttributeNameKey::from(7))]);
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
        assert!(!same_change(&updated(1), &second));
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
        let dictionary = HashMap::from([(speed(), AttributeNameKey::from(7))]);
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
        let dictionary = HashMap::from([(speed(), AttributeNameKey::from(7))]);
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
        assert!(event(WriteOrdinal::at(0), 0, group, &HashMap::new()).is_err());
    }
}
