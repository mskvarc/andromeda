//! Who one committed change is evaluated for, and the candidates its entity
//! filters admit for them (clause 5.8.6).

use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change_filters::{ChangedStates, HopResolution, narrow_by_change_filters},
    compiled::CompiledSubscription,
    notifying_since::SelectionJudged,
};
use chrono::Utc;
use std::sync::Arc;
use tracing::debug;

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

impl ChangeAudience<'_> {
    /// Whether `candidate`'s selection is the one its commit matched the change
    /// against, or the one standing when the change is evaluated here.
    ///
    /// A live change is matched only here. A queued delivery was matched at
    /// commit, its entity filters with it unless the commit could not decide
    /// them or they hop into linked entities, which are both evaluated here.
    pub(crate) fn selection_judged(self, candidate: &CompiledSubscription) -> SelectionJudged {
        match self {
            ChangeAudience::Live
            | ChangeAudience::Queued {
                filters: CommitFilters::Undecided,
                ..
            } => SelectionJudged::AtEvaluation,
            ChangeAudience::Queued {
                filters: CommitFilters::Decided,
                ..
            } => {
                if candidate.filters_hop() {
                    SelectionJudged::AtEvaluation
                } else {
                    SelectionJudged::AtCommit
                }
            }
        }
    }
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

/// The subscriptions `audience` evaluates the change for, before their entity
/// filters and the instants they notify since are read.
///
/// A live change is routed now through every subscription's `entities`,
/// `notificationTrigger` and `watchedAttributes`. A queued delivery was routed
/// by the commit that queued it, under the selection in force then (clauses
/// 5.8.2 and 5.8.6 owe the notification from the change itself), so the one
/// subscription it is addressed to is taken as it now stands, and only
/// whether it is still notified of changes is read: an update that moved what
/// it selects since does not withdraw the notification already owed, while a
/// subscription deleted, paused, expired or turned periodic since is notified
/// of nothing.
pub(crate) fn addressed(
    audience: ChangeAudience<'_>,
    cache: &SubscriptionCache,
    target: &ContextTarget,
    event: &ChangeEvent,
) -> Vec<Arc<CompiledSubscription>> {
    match audience {
        ChangeAudience::Live => {
            let outcome = cache.find_matching(target, event);
            debug!(
                entity = %event.entity_id,
                triggers = ?event.change.triggers(),
                active = outcome.funnel.active,
                routed = outcome.funnel.routed,
                watched = outcome.funnel.watched,
                evaluated = outcome.funnel.evaluated,
                matched = outcome.funnel.matched,
                "Subscription candidate funnel"
            );
            outcome.funnel.record();
            outcome.matched
        }
        ChangeAudience::Queued { subscription, .. } => cache
            .get(target, subscription)
            .filter(|candidate| candidate.notifies_on_change_at(Utc::now()))
            .into_iter()
            .collect(),
    }
}

/// The candidates a change committed at `committed_at` is owed to: each one
/// active when the change committed and, unless its commit matched it, selecting
/// as it did then (clause 5.8.6).
pub(crate) fn owed_since_commit(
    audience: ChangeAudience<'_>,
    committed_at: StoreInstant,
    mut candidates: Vec<Arc<CompiledSubscription>>,
) -> Vec<Arc<CompiledSubscription>> {
    candidates.retain(|candidate| {
        let owed = candidate.notifying_since.owes(committed_at, audience.selection_judged(candidate));
        if !owed {
            debug!(subscription = %candidate.id, "Subscription skipped: it became active or reselected after the change committed");
        }
        owed
    });
    candidates
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
/// change is evaluated against the entity the write left behind, and, for a
/// subscription its removed instances are owed to, against the state before it
/// as well.
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
