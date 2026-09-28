//! The in-memory index that narrows active subscriptions to the candidates one
//! change event can match.
//!
//! Every stage here is a superset filter: it must never drop a subscription
//! [`matches_subscription`] would accept, and it may keep ones it would
//! reject. The matcher stays authoritative; the index only decides how many
//! subscriptions it has to look at.

use crate::{
    candidate_funnel::MatchOutcome,
    compiled::CompiledSubscription,
    matching::matches_subscription,
    membership::Membership,
    selector_routing::{SelectorRef, SelectorRouting, SelectorShape},
};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_key::SubscriptionKey};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_telemetry::subscription::CandidateFunnel;
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
};
use strum::EnumCount;

/// The routing buckets over one tenant's active subscriptions.
///
/// Held behind one lock by the cache: a matching pass reads several buckets
/// and must see them from the same generation, and a mutation touches several
/// buckets that must move together.
#[derive(Debug, Default)]
pub struct CandidateIndex {
    entries: HashMap<SubscriptionKey, Arc<CompiledSubscription>>,
    keys_by_id: HashMap<Uri, SubscriptionKey>,
    by_type: HashMap<EntityTypeName, HashSet<SelectorRef>>,
    by_entity_id: HashMap<Uri, HashSet<SelectorRef>>,
    wildcard: HashSet<SubscriptionKey>,
    by_trigger: [HashSet<SubscriptionKey>; NotificationTrigger::COUNT],
    by_watched_attribute: HashMap<AttributeName, HashSet<SubscriptionKey>>,
    watch_all: HashSet<SubscriptionKey>,
    membership: HashMap<SubscriptionKey, Membership>,
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
    pub fn get(&self, id: &Uri) -> Option<&Arc<CompiledSubscription>> {
        self.keys_by_id.get(id).and_then(|key| self.entries.get(key))
    }

    /// Drop every subscription.
    pub fn clear(&mut self) {
        *self = CandidateIndex::default();
    }

    /// Index a compiled subscription, replacing any earlier entry with the same
    /// identifier.
    pub fn insert(&mut self, compiled: Arc<CompiledSubscription>) {
        self.remove(&compiled.id);
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

    /// Unindex the subscription with this identifier, walking only the
    /// buckets its membership record names.
    pub fn remove(&mut self, id: &Uri) -> Option<Arc<CompiledSubscription>> {
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

    fn drop_selectors<K: Hash + Eq>(buckets: &mut HashMap<K, HashSet<SelectorRef>>, bucket: &K, key: SubscriptionKey) {
        if let Some(refs) = buckets.get_mut(bucket) {
            refs.retain(|selector| selector.subscription != key);
            if refs.is_empty() {
                buckets.remove(bucket);
            }
        }
    }

    fn drop_member<K: Hash + Eq>(buckets: &mut HashMap<K, HashSet<SubscriptionKey>>, bucket: &K, key: SubscriptionKey) {
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
        let raised = event.change.triggers();
        let watched: Vec<SubscriptionKey> = routed
            .into_iter()
            .filter(|key| raised.iter().any(|trigger| self.by_trigger[trigger_slot(*trigger)].contains(key)))
            .filter(|key| self.admits_changed_attributes(*key, event))
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

    /// Selector stage: the wildcard bucket, plus every selector whose indexed
    /// type and/or id the event hits in the combination its shape demands.
    fn route_selectors(&self, event: &ChangeEvent) -> HashSet<SubscriptionKey> {
        let mut routed: HashSet<SubscriptionKey> = self.wildcard.iter().copied().collect();
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

    /// Watched-attribute stage: a subscription without `watchedAttributes`
    /// admits every event; one with them admits an event that changed at least
    /// one of them.
    fn admits_changed_attributes(&self, key: SubscriptionKey, event: &ChangeEvent) -> bool {
        self.watch_all.contains(&key)
            || event
                .change
                .attributes()
                .into_iter()
                .any(|name| self.by_watched_attribute.get(name).is_some_and(|keys| keys.contains(&key)))
    }
}
