//! Which candidate subscriptions an entity change satisfies the entity filters
//! of (clause 5.8.6).
//!
//! The subscribed Entities "that changed and that match [...] the query,
//! geoquery and Scope query conditions" are read after the write -- except an
//! Entity the write deleted, which only its previous state describes. A change
//! that removed instances also matches through the state before the write, so a
//! subscription filtering on a removed Attribute still hears of its removal.
//! The durable path that queues a notification in the write's transaction and
//! the live path that sends it at once both decide it here.

use crate::{cache::SubscriptionCache, compiled::CompiledSubscription, matching::matches_entity_filters, spatial_narrowing::narrow_by_spatial_index};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    expanded::attribute_expiry::standing_at,
};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

/// The states of the Entity an entity change is filtered against.
#[derive(Clone, Copy, Debug)]
pub struct ChangedStates<'a> {
    /// The Entity as the write left it, absent for a deletion.
    pub current: Option<&'a Value>,
    /// The Entity as it stood before the write, absent for a creation.
    pub previous: Option<&'a Value>,
}

/// The candidates whose `q`, `geoQ` and `scopeQ` hold on a state the change is
/// filtered against, in candidate order.
///
/// With no state to read, no filter can be evaluated and the candidates stand.
/// Either state is read as it stands now, without its expired instances
/// (clause 4.22).
#[must_use]
pub fn narrow_by_change_filters(
    cache: &SubscriptionCache,
    target: &ContextTarget,
    candidates: Vec<Arc<CompiledSubscription>>,
    change: &EntityChange,
    states: ChangedStates<'_>,
    entity_types: &[EntityTypeName],
) -> Vec<Arc<CompiledSubscription>> {
    let filtered: Vec<&Value> = match change.lifecycle {
        EntityLifecycle::Deleted => states.previous.or(states.current).into_iter().collect(),
        EntityLifecycle::Created | EntityLifecycle::Modified => states
            .current
            .into_iter()
            .chain(states.previous.filter(|_| change.deletes_instances()))
            .collect(),
    };
    if filtered.is_empty() {
        return candidates;
    }
    let now = Utc::now();
    let mut matching: Vec<Arc<CompiledSubscription>> = Vec::with_capacity(candidates.len());
    for state in filtered.into_iter().map(|state| standing_at(state, now)) {
        let remaining: Vec<Arc<CompiledSubscription>> = candidates
            .iter()
            .filter(|candidate| !matching.iter().any(|kept| kept.key == candidate.key))
            .map(Arc::clone)
            .collect();
        matching.extend(
            narrow_by_spatial_index(cache, target, remaining, &state, entity_types)
                .into_iter()
                .filter(|subscription| matches_entity_filters(subscription, &state)),
        );
    }
    matching
}
