//! The in-memory index that narrows active subscriptions to the candidates one
//! change event can match.
//!
//! Every stage here is a superset filter: it must never drop a subscription
//! [`matches_subscription`] would accept, and it may keep ones it would
//! reject. The matcher stays authoritative; the index only decides how many
//! subscriptions it has to look at.
//!
//! Beside the subscriptions in force, the index keeps the selections an update
//! superseded, under the same lock, so one matching pass sees either the
//! version in force at a change's commit or the one that superseded it, never
//! both and never neither.

use crate::{
    candidate_funnel::MatchOutcome,
    compiled::CompiledSubscription,
    matching::{matches_subscription, selects_change},
    membership::Membership,
    selector_routing::{SelectorRef, SelectorRouting, SelectorShape},
    superseded_selection::SupersededSelections,
};
use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant, subscription_key::SubscriptionKey};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_telemetry::subscription::CandidateFunnel;
use chrono::Utc;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{collections::HashMap, hash::Hash, ptr, sync::Arc};
use strum::EnumCount;

/// The routing buckets over one tenant's active subscriptions.
///
/// Held behind one lock by the cache: a matching pass reads several buckets
/// and must see them from the same generation, and a mutation touches several
/// buckets that must move together.
#[derive(Debug, Default)]
pub struct CandidateIndex {
    entries: FxHashMap<SubscriptionKey, Arc<CompiledSubscription>>,
    keys_by_id: HashMap<Identifier, SubscriptionKey>,
    by_type: HashMap<EntityTypeName, FxHashSet<SelectorRef>>,
    by_entity_id: HashMap<Identifier, FxHashSet<SelectorRef>>,
    wildcard: FxHashSet<SubscriptionKey>,
    by_trigger: [FxHashSet<SubscriptionKey>; NotificationTrigger::COUNT],
    by_watched_attribute: HashMap<AttributeName, FxHashSet<SubscriptionKey>>,
    watch_all: FxHashSet<SubscriptionKey>,
    membership: FxHashMap<SubscriptionKey, Membership>,
    /// The selections updates replaced while a change committed under one may
    /// still be evaluated.
    superseded: SupersededSelections,
}

/// The slot of a trigger in the per-trigger bucket array.
const fn trigger_slot(trigger: NotificationTrigger) -> usize {
    match trigger {
        NotificationTrigger::EntityCreated => 0,
        NotificationTrigger::EntityUpdated => 1,
        NotificationTrigger::EntityDeleted => 2,
        NotificationTrigger::AttributeCreated => 3,
        NotificationTrigger::AttributeUpdated => 4,
        NotificationTrigger::AttributeDeleted => 5,
    }
}

impl CandidateIndex {
    /// Number of indexed subscriptions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no subscription is indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The indexed subscription with this identifier.
    #[must_use]
    pub fn get(&self, id: &Identifier) -> Option<&Arc<CompiledSubscription>> {
        self.keys_by_id.get(id).and_then(|key| self.entries.get(key))
    }

    /// Drop every subscription.
    pub fn clear(&mut self) {
        *self = CandidateIndex::default();
    }

    /// Index a compiled subscription, replacing any earlier entry with the same
    /// identifier; an entry whose selection it changes is kept as superseded.
    pub fn insert(&mut self, compiled: Arc<CompiledSubscription>) {
        if let Some(previous) = self.unindex(&compiled.id) {
            self.superseded.supersede(previous, &compiled);
        }
        let key = compiled.key;
        let selectors = SelectorRouting::of(&compiled);
        if selectors.wildcard {
            self.wildcard.insert(key);
        }
        for (type_name, selector) in &selectors.by_type {
            self.by_type.entry(type_name.clone()).or_default().insert(*selector); // clone: bucket key
        }
        for (id, selector) in &selectors.by_entity_id {
            self.by_entity_id.entry(id.clone()).or_default().insert(*selector); // clone: bucket key
        }
        for trigger in &compiled.triggers {
            self.by_trigger[trigger_slot(*trigger)].insert(key);
        }
        let watched_attributes = if let Some(watched) = &compiled.watched_attrs {
            for name in watched {
                self.by_watched_attribute.entry(name.clone()).or_default().insert(key); // clone: bucket key
            }
            watched.iter().cloned().collect() // clone: the reverse map owns its bucket names
        } else {
            self.watch_all.insert(key);
            Vec::new()
        };
        self.membership.insert(
            key,
            Membership {
                triggers: compiled.triggers.clone(), // clone: the reverse map owns the trigger list it will unindex
                watch_all: compiled.watched_attrs.is_none(),
                watched_attributes,
                selectors,
            },
        );
        self.keys_by_id.insert(compiled.id.clone(), key); // clone: the map owns its key
        self.entries.insert(key, compiled);
    }

    /// Unindex the subscription with this identifier and drop the selections
    /// it superseded: it is no longer notified of changes (clause 5.8.6).
    pub fn remove(&mut self, id: &Identifier) -> Option<Arc<CompiledSubscription>> {
        self.superseded.forget(id);
        self.unindex(id)
    }

    /// Unindex the subscription in force with this identifier, walking only
    /// the buckets its membership record names.
    fn unindex(&mut self, id: &Identifier) -> Option<Arc<CompiledSubscription>> {
        let key = self.keys_by_id.remove(id)?;
        let compiled = self.entries.remove(&key);
        if let Some(membership) = self.membership.remove(&key) {
            self.wildcard.remove(&key);
            for type_name in membership.type_buckets() {
                Self::drop_selectors(&mut self.by_type, type_name, key);
            }
            for entity_id in membership.id_buckets() {
                Self::drop_selectors(&mut self.by_entity_id, entity_id, key);
            }
            for trigger in membership.triggers {
                self.by_trigger[trigger_slot(trigger)].remove(&key);
            }
            for name in &membership.watched_attributes {
                Self::drop_member(&mut self.by_watched_attribute, name, key);
            }
            self.watch_all.remove(&key);
        }
        compiled
    }

    fn drop_selectors<K: Hash + Eq>(buckets: &mut HashMap<K, FxHashSet<SelectorRef>>, bucket: &K, key: SubscriptionKey) {
        if let Some(refs) = buckets.get_mut(bucket) {
            refs.retain(|selector| selector.subscription != key);
            if refs.is_empty() {
                buckets.remove(bucket);
            }
        }
    }

    fn drop_member<K: Hash + Eq>(buckets: &mut HashMap<K, FxHashSet<SubscriptionKey>>, bucket: &K, key: SubscriptionKey) {
        if let Some(keys) = buckets.get_mut(bucket) {
            keys.remove(&key);
            if keys.is_empty() {
                buckets.remove(bucket);
            }
        }
    }

    /// Route one event through the funnel and evaluate the survivors with the
    /// authoritative matcher.
    ///
    /// The trigger stage keeps a subscription listening to any trigger the
    /// change raises; the matcher then decides whether one of them was raised
    /// on a watched attribute.
    pub fn find_matching(&self, event: &ChangeEvent) -> MatchOutcome {
        let routed = self.route_selectors(event);
        let routed_count = routed.len();
        let listening: Vec<&FxHashSet<SubscriptionKey>> = event
            .change
            .triggers()
            .into_iter()
            .map(|trigger| &self.by_trigger[trigger_slot(trigger)])
            .collect();
        let watching = self.watching_changed_attributes(event);
        let watched: Vec<SubscriptionKey> = routed
            .into_iter()
            .filter(|key| listening.iter().any(|keys| keys.contains(key)))
            .filter(|key| self.watch_all.contains(key) || watching.iter().any(|keys| keys.contains(key)))
            .collect();
        let candidates: Vec<&Arc<CompiledSubscription>> = watched.iter().filter_map(|key| self.entries.get(key)).collect();
        let evaluated = candidates.len();
        let matched: Vec<Arc<CompiledSubscription>> = candidates
            .into_iter()
            .filter(|subscription| matches_subscription(subscription, event))
            .map(Arc::clone)
            .collect();
        MatchOutcome {
            funnel: CandidateFunnel {
                active: self.entries.len(),
                routed: routed_count,
                watched: watched.len(),
                evaluated,
                matched: matched.len(),
            },
            matched,
        }
    }

    /// The superseded selections that stood when a change committed at
    /// `committed_at` and select `event`, each of a subscription still notified
    /// of changes as it now stands.
    ///
    /// The superseded selections are few and live only while evaluation lags
    /// behind an update, so each is matched directly rather than routed.
    pub fn find_superseded_matching(&self, event: &ChangeEvent, committed_at: StoreInstant) -> Vec<Arc<CompiledSubscription>> {
        let now = Utc::now();
        self.superseded
            .standing_at(committed_at)
            .filter(|selection| self.get(&selection.id).is_some_and(|current| current.notifies_on_change_at(now)))
            .filter(|selection| selects_change(selection, event))
            .map(Arc::clone)
            .collect()
    }

    /// Whether `subscription` is the version in force rather than a superseded
    /// one.
    #[must_use]
    pub fn is_in_force(&self, subscription: &CompiledSubscription) -> bool {
        self.get(&subscription.id).is_some_and(|current| ptr::eq(current.as_ref(), subscription))
    }

    /// Whether a superseded selection is no longer awaited by any change
    /// committed at or after `floor`, the earliest commit instant evaluation
    /// still holds (`None` when it holds none).
    #[must_use]
    pub fn releases_superseded(&self, floor: Option<StoreInstant>) -> bool {
        self.superseded.releases(floor)
    }

    /// Drop every superseded selection no change committed at or after
    /// `floor` can have committed under.
    pub fn release_superseded(&mut self, floor: Option<StoreInstant>) {
        self.superseded.release(floor);
    }

    /// Number of superseded selections kept.
    #[must_use]
    pub const fn superseded_len(&self) -> usize {
        self.superseded.len()
    }

    /// Selector stage: the wildcard bucket, plus every selector whose indexed
    /// type and/or id the event hits in the combination its shape demands.
    fn route_selectors(&self, event: &ChangeEvent) -> FxHashSet<SubscriptionKey> {
        let mut routed: FxHashSet<SubscriptionKey> = self.wildcard.iter().copied().collect();
        let id_hits = self.by_entity_id.get(&event.entity_id);
        for selector in event.entity_types.iter().filter_map(|type_name| self.by_type.get(type_name)).flatten() {
            match selector.shape {
                SelectorShape::TypeOnly => {
                    routed.insert(selector.subscription);
                }
                SelectorShape::TypeAndId => {
                    if id_hits.is_some_and(|hits| hits.contains(selector)) {
                        routed.insert(selector.subscription);
                    }
                }
                SelectorShape::IdOnly => {}
            }
        }
        for selector in id_hits.into_iter().flatten() {
            match selector.shape {
                SelectorShape::IdOnly => {
                    routed.insert(selector.subscription);
                }
                SelectorShape::TypeOnly | SelectorShape::TypeAndId => {}
            }
        }
        routed
    }

    /// Watched-attribute stage, looked up once per event: the buckets of the
    /// subscriptions watching an attribute the event changed. A subscription
    /// without `watchedAttributes` admits every event; one with them admits an
    /// event whose changed attributes land it in one of these buckets.
    fn watching_changed_attributes(&self, event: &ChangeEvent) -> Vec<&FxHashSet<SubscriptionKey>> {
        event
            .change
            .attributes()
            .into_iter()
            .filter_map(|name| self.by_watched_attribute.get(name))
            .collect()
    }
}
