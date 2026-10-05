//! Who one committed change is evaluated for, and the candidates its entity
//! filters admit for them (clause 5.8.6).

use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change_filters::{ChangedStates, HopResolution, narrow_by_change_filters},
    compiled::CompiledSubscription,
};
use std::sync::Arc;

/// Who one committed change is evaluated for.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ChangeAudience<'a> {
    /// Every subscription the change reaches, its entity filters decided now.
    Live,
    /// The one subscription a durable delivery is addressed to.
    Queued {
        /// The subscription the delivery is addressed to.
        subscription: &'a Identifier,
        /// Whether the commit already decided its entity filters.
        filters: CommitFilters,
    },
}

/// Whether the commit that queued a delivery decided the subscription's
/// entity filters.
///
/// A delivery is queued because the commit found the change matching the
/// subscription's `q`, `geoQ`, `scopeQ` and `csf` on the very states queued
/// with it, and clause 5.8.6 decides a notification by the change and the
/// state it left, both fixed at commit, so those filters are not evaluated a
/// second time. What may have moved since is the subscription itself, which is
/// read again, and the instants of the entity's instances, which the rendering
/// reads as they stand at delivery (clause 4.22).
#[derive(Clone, Copy, Debug)]
pub(crate) enum CommitFilters {
    /// The commit evaluated the filters on a state queued with the delivery.
    Decided,
    /// The commit had no state to evaluate them on, so it admitted the change
    /// and delivery evaluates them on the entity it reads.
    Undecided,
}

/// The candidates the change's entity filters admit for `audience`: decided
/// now for a live change, as the commit decided them for a queued delivery.
///
/// The commit cannot read the entities a `rel{…}` hop crosses into, so it
/// records a subscription whose filters hop undecided; its queued delivery
/// decides every filter of it here, on the states queued with it.
pub(crate) fn admitted_by_entity_filters(
    audience: ChangeAudience<'_>,
    cache: &SubscriptionCache,
    target: &ContextTarget,
    event: &ChangeEvent,
    candidates: Vec<Arc<CompiledSubscription>>,
    states: ChangedStates<'_>,
    linked: &LinkedStates,
) -> Vec<Arc<CompiledSubscription>> {
    let narrow = |candidates| {
        narrow_by_change_filters(
            cache,
            target,
            candidates,
            &event.change,
            states,
            &event.entity_types,
            HopResolution::Resolved(linked),
        )
    };
    match audience {
        ChangeAudience::Live
        | ChangeAudience::Queued {
            filters: CommitFilters::Undecided,
            ..
        } => narrow(candidates),
        ChangeAudience::Queued {
            filters: CommitFilters::Decided,
            ..
        } => {
            let (hopping, decided): (Vec<_>, Vec<_>) = candidates.into_iter().partition(|candidate| candidate.filters_hop());
            decided.into_iter().chain(narrow(hopping)).collect()
        }
    }
}

/// The states the change's entity filters read (clause 5.8.6), exactly as the
/// durable path reads them at commit.
///
/// A deletion is evaluated against what the entity was, because what it is
/// now is nothing; the state before the write is what the event carries as
/// its previous attributes, or, failing that, the removed state. Every other
/// change is evaluated against the entity the write left behind, and against
/// the state before it as well when it removed instances.
pub(crate) fn changed_states<'a>(event: &'a ChangeEvent, notified: Option<&'a EntityState>) -> ChangedStates<'a> {
    let previous = event.previous_attributes.as_deref();
    match event.change.lifecycle {
        EntityLifecycle::Deleted => ChangedStates {
            current: None,
            previous: previous.or(event.entity_snapshot.as_deref()),
        },
        EntityLifecycle::Created | EntityLifecycle::Modified => ChangedStates { current: notified, previous },
    }
}
