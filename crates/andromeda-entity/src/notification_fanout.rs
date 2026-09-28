//! Turning one entity mutation into the subscriptions it matched.
//!
//! Matching happens in memory against the shared subscription cache, then the
//! entity-level filters (`q`, `geoQ`, `scopeQ`) narrow it further. The records
//! are written in the same transaction as the mutation that raised them, which
//! is what makes a notification crash-consistent with the change it describes.

use crate::{NotificationWriteConfig, change_notifier::ChangeNotifier};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::ChangeOrigin};
use andromeda_subscription::change_filters::{ChangedStates, narrow_by_change_filters};
use chrono::Utc;
use serde_json::Value;
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
    pub id: &'a Uri,
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
    /// The entity as it stands after the mutation, absent when the mutation
    /// carries no rendered state.
    pub current: Option<&'a Value>,
    /// The entity as it stood before the mutation, absent when there was no
    /// previous state.
    pub previous: Option<&'a Value>,
}

impl ChangeNotifier {
    /// Build one record for every subscription `change` is owed to.
    ///
    /// Returns an empty vector when notification recording is not configured:
    /// a live-delivery broker's notification worker evaluates the change itself.
    pub(crate) fn subscription_matches(&self, entity: ChangedEntity<'_>, change: &EntityChange, snapshots: MutationSnapshots<'_>) -> Vec<SubscriptionMatch> {
        self.recording
            .as_ref()
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
            current: entity_snapshot,
            previous: previous_snapshot,
        } = snapshots;

        let event = ChangeEvent::builder()
            .entity_id(entity_id.clone()) // clone: owned Uri for ChangeEvent
            .entity_types(entity_types.to_vec())
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

        let states = ChangedStates {
            current: entity_snapshot,
            previous: previous_snapshot,
        };
        let matching = narrow_by_change_filters(subscription_cache, target, outcome.matched, change, states, entity_types);
        matching
            .into_iter()
            .map(|sub| {
                // clone: every entry owns its own copy of the entity id, the
                // change and the snapshots, all of which are borrowed here.
                SubscriptionMatch::builder()
                    .subscription_key(sub.key)
                    .entity_id(entity_id.clone())
                    .change(change.clone())
                    .entity_snapshot(entity_snapshot.cloned())
                    .previous_snapshot(previous_snapshot.cloned())
                    .build()
            })
            .collect()
    }
}
