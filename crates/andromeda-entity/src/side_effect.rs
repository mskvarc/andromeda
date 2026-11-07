//! Work a batch write produces but does not have to finish before it answers.
//!
//! Temporal history and change events are the two, and Andromeda lets a
//! client trade waiting for them for latency with `Prefer: respond-async`.
//! Either they run inline in the request that caused them, or they are
//! persisted alongside the entity write: the history as queued intents, which
//! every temporal read answers at once, and the change events as a job a
//! worker drains.

use andromeda_context_space::context_target::ContextTarget;

/// The live branch, for the placement that can only ever write to it.
static LIVE_BRANCH: ContextTarget = ContextTarget::Live;

/// The branch a batch writes and where its side effects run.
///
/// Only the live branch's side-effect queue is drained, so only a batch on the
/// live branch may defer its side effects. The two are one value rather than a
/// target and a mode passed side by side, which is what keeps a deferred batch
/// on a Snapshot branch from being expressed at all: a job queued there would
/// never run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideEffectPlacement<'a> {
    /// Run side effects inline in the request that triggered them, on this
    /// branch: the batch response implies the temporal history and any
    /// derived notifications have already been produced.
    Inline(&'a ContextTarget),
    /// Write the live branch, its history as queued intents, and persist the
    /// side-effect job in the same transaction as the entity write; a
    /// background worker executes the job at-least-once. Opt-in via `Prefer:
    /// respond-async` plus the configured minimum-batch-size threshold.
    DeferredOnLive,
}

impl<'a> SideEffectPlacement<'a> {
    /// The branch the batch writes.
    #[must_use]
    pub const fn target(self) -> &'a ContextTarget {
        match self {
            SideEffectPlacement::Inline(target) => target,
            SideEffectPlacement::DeferredOnLive => &LIVE_BRANCH,
        }
    }
}

/// Whether the request opted into `Prefer: respond-async` processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncPreference {
    /// The request explicitly opted into asynchronous processing.
    Requested,
    /// The request did not opt into asynchronous processing.
    NotRequested,
}

/// Pure routing logic for
/// [`EntityService::side_effect_placement`](crate::EntityService::side_effect_placement).
///
/// Split from the method so the decision table can be unit-tested without
/// constructing a full service.
pub(crate) const fn resolve_side_effect_placement(
    target: &ContextTarget,
    async_threshold: usize,
    preference: AsyncPreference,
    batch_size: usize,
) -> SideEffectPlacement<'_> {
    match (target, preference) {
        (ContextTarget::Live, AsyncPreference::Requested) if batch_size >= async_threshold => SideEffectPlacement::DeferredOnLive,
        (ContextTarget::Live | ContextTarget::Snapshot(_), AsyncPreference::Requested | AsyncPreference::NotRequested) => SideEffectPlacement::Inline(target),
    }
}

#[cfg(test)]
mod tests {
    use crate::side_effect::{AsyncPreference, SideEffectPlacement, resolve_side_effect_placement};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};

    fn snapshot() -> ContextTarget {
        ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(7).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:inline".parse().expect("a valid snapshot URI"),
        ))
    }

    /// Only the live branch's side-effect queue is drained, so a batch scoped
    /// to a Snapshot (clause 5.5.15) runs its side effects inline, on that
    /// Snapshot, even when it asked to respond asynchronously and passes the
    /// threshold.
    #[test]
    fn a_snapshot_scoped_batch_runs_its_side_effects_inline_on_its_snapshot_even_when_async_is_requested() {
        let snapshot = snapshot();
        for threshold in [1, 10] {
            pretty_assertions::assert_eq!(
                resolve_side_effect_placement(&snapshot, threshold, AsyncPreference::Requested, 10),
                SideEffectPlacement::Inline(&snapshot),
                "threshold {threshold}"
            );
        }
    }

    /// A deferred placement writes the live branch, and an inline one writes
    /// the branch it was resolved for.
    #[test]
    fn a_placement_writes_the_branch_it_was_resolved_for() {
        let snapshot = snapshot();
        pretty_assertions::assert_eq!(SideEffectPlacement::DeferredOnLive.target(), &ContextTarget::Live);
        pretty_assertions::assert_eq!(SideEffectPlacement::Inline(&snapshot).target(), &snapshot);
        pretty_assertions::assert_eq!(SideEffectPlacement::Inline(&ContextTarget::Live).target(), &ContextTarget::Live);
    }

    #[test]
    fn prefer_respond_async_with_a_threshold_of_one_defers_every_batch_on_the_live_branch() {
        pretty_assertions::assert_eq!(
            resolve_side_effect_placement(&ContextTarget::Live, 1, AsyncPreference::Requested, 1),
            SideEffectPlacement::DeferredOnLive
        );
    }

    #[test]
    fn prefer_respond_async_with_threshold_above_batch_size_runs_inline() {
        pretty_assertions::assert_eq!(
            resolve_side_effect_placement(&ContextTarget::Live, 100, AsyncPreference::Requested, 10),
            SideEffectPlacement::Inline(&ContextTarget::Live)
        );
    }

    #[test]
    fn prefer_respond_async_at_or_above_threshold_defers_on_the_live_branch() {
        for batch_size in [100, 512] {
            pretty_assertions::assert_eq!(
                resolve_side_effect_placement(&ContextTarget::Live, 100, AsyncPreference::Requested, batch_size),
                SideEffectPlacement::DeferredOnLive,
                "batch size {batch_size}"
            );
        }
    }

    #[test]
    fn without_the_preference_a_batch_runs_inline_at_the_lowest_threshold() {
        pretty_assertions::assert_eq!(
            resolve_side_effect_placement(&ContextTarget::Live, 1, AsyncPreference::NotRequested, 1000),
            SideEffectPlacement::Inline(&ContextTarget::Live)
        );
    }

    #[test]
    fn no_async_opt_in_runs_inline_regardless_of_threshold() {
        for threshold in [1, 100] {
            pretty_assertions::assert_eq!(
                resolve_side_effect_placement(&ContextTarget::Live, threshold, AsyncPreference::NotRequested, 1000),
                SideEffectPlacement::Inline(&ContextTarget::Live),
                "threshold {threshold}"
            );
        }
    }
}
