//! Turning one entity mutation into the subscriptions it matched.
//!
//! Matching happens in memory against the shared subscription cache, then the
//! entity-level filters (`q`, `geoQ`, `scopeQ`) narrow it further. The records
//! are written in the same transaction as the mutation that raised them, which
//! is what makes a notification crash-consistent with the change it describes.

use crate::{NotificationWriteConfig, change_notifier::ChangeNotifier, deletion_outline::DeletionWatchers, snapshot_supply::SnapshotSupply};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::ChangeOrigin};
use andromeda_subscription::change_filters::{ChangedStates, narrow_by_change_filters};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

/// The entity a mutation changed, as subscription matching addresses it.
///
/// The three values are one identity rather than three parameters: a
/// subscription is selected for a mutation by the context branch it watches,
/// the entity identifier and the entity types (clause 5.2.12), and none of them
/// selects anything on its own.
#[derive(Clone, Copy)]
pub(crate) struct ChangedEntity<'a> {
    /// The context branch the mutation landed in.
    pub target: &'a ContextTarget,
    /// The identifier of the changed entity.
    pub id: &'a Identifier,
    /// The types a subscription's `entities` selector is matched against.
    pub types: &'a [EntityTypeName],
}

/// The entity states a matched subscription is filtered and rendered against.
///
/// Both states travel together because which one a filter reads depends on the
/// trigger: a creation or an update is filtered on the state the mutation
/// produced, a deletion on the state it removed (clause 5.2.12). The previous
/// state is also what a `showChanges` notification reports.
#[derive(Clone, Copy)]
pub(crate) struct MutationSnapshots<'a> {
    /// The entity a matched subscription is filtered and notified with: the
    /// state the mutation produced, or for a deletion the state it removed.
    pub current: SnapshotSupply<'a>,
    /// The entity as it stood before the mutation, absent when there was no
    /// previous state.
    pub previous: Option<&'a Arc<Value>>,
}

impl ChangeNotifier {
    /// Whether this process's subscription cache holds a candidate for the
    /// deletion `change` describes, so the state it removes must be read for
    /// the notification.
    ///
    /// Asked whichever way this broker delivers: a durable match is recorded
    /// at commit and a live one is matched by the notification worker against
    /// the published change, and both render the removed state. The cache can
    /// hold a subscription the shared store's probe does not yet show this
    /// transaction, and a candidate here is reason enough to read.
    pub(crate) fn cached_deletion_watchers(&self, entity: ChangedEntity<'_>, change: &EntityChange) -> DeletionWatchers {
        let Some(subscription_cache) = self.subscription_cache() else {
            return DeletionWatchers::None;
        };
        let event = ChangeEvent::builder()
            .entity_id(entity.id.clone()) // clone: owned Identifier for ChangeEvent
            .entity_types(entity.types.to_vec()) // clone: the matched event owns the types it is matched by
            .change(change.clone()) // clone: the matched event owns the change it is matched as
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build();
        if subscription_cache.find_matching(entity.target, &event).matched.is_empty() {
            DeletionWatchers::None
        } else {
            DeletionWatchers::Possible
        }
    }

    /// Build one record for every subscription `change` is owed to.
    ///
    /// Returns an empty vector when notification recording is not configured:
    /// a live-delivery broker's notification worker evaluates the change itself.
    pub(crate) fn subscription_matches(&self, entity: ChangedEntity<'_>, change: &EntityChange, snapshots: MutationSnapshots<'_>) -> Vec<SubscriptionMatch> {
        self.recording()
            .map_or_else(Vec::new, |recording| recording.subscription_matches(entity, change, snapshots))
    }
}

impl NotificationWriteConfig {
    /// Build one durable match for every subscription one entity change is owed
    /// to (clause 5.8.6): at most one per subscription, however many of its
    /// triggers the change raised or instances it touched.
    pub(crate) fn subscription_matches(&self, entity: ChangedEntity<'_>, change: &EntityChange, snapshots: MutationSnapshots<'_>) -> Vec<SubscriptionMatch> {
        let NotificationWriteConfig { subscription_cache } = self;
        let ChangedEntity {
            target,
            id: entity_id,
            types: entity_types,
        } = entity;
        let MutationSnapshots {
            current: current_supply,
            previous: previous_snapshot,
        } = snapshots;

        let event = ChangeEvent::builder()
            .entity_id(entity_id.clone()) // clone: owned Identifier for ChangeEvent
            .entity_types(entity_types.to_vec()) // clone: the matched event owns the types it is matched by
            .change(change.clone()) // clone: the matched event owns the change it is matched as
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build();

        let outcome = subscription_cache.find_matching(target, &event);
        debug!(
            entity = %entity_id,
            triggers = ?change.triggers(),
            active = outcome.funnel.active,
            routed = outcome.funnel.routed,
            watched = outcome.funnel.watched,
            evaluated = outcome.funnel.evaluated,
            matched = outcome.funnel.matched,
            "Subscription candidate funnel"
        );
        outcome.funnel.record();
        // A change no subscription is a candidate for owes nothing, and the
        // state a match is notified with is not built for it.
        if outcome.matched.is_empty() {
            return Vec::new();
        }

        let entity_snapshot = current_supply.materialize();
        let states = ChangedStates {
            current: entity_snapshot.as_deref(),
            previous: previous_snapshot.map(|previous| &**previous),
        };
        let matching = narrow_by_change_filters(subscription_cache, target, outcome.matched, change, states, entity_types);
        matching
            .into_iter()
            .map(|sub| {
                // clone: every entry owns its own copy of the entity id and the
                // change; the snapshots are shared by reference count.
                SubscriptionMatch::builder()
                    .subscription_key(sub.key)
                    .entity_id(entity_id.clone())
                    .change(change.clone())
                    .entity_snapshot(entity_snapshot.as_ref().map(Arc::clone))
                    .previous_snapshot(previous_snapshot.map(Arc::clone))
                    .build()
            })
            .collect()
    }
}
