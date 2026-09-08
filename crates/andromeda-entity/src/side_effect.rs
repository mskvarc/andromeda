//! Work a batch write produces but does not have to finish before it answers.
//!
//! Temporal history and change events are the two, and Andromeda lets a
//! client trade read-your-write for latency on them with `Prefer:
//! respond-async`. Either they run inline in the request that caused them, or
//! they are persisted alongside the entity write and drained by a worker.

/// How a request's side effects are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSideEffectMode {
    /// Run side-effects inline in the same request that triggered them.
    /// Strong read-your-write: the batch response implies the temporal
    /// history and any derived notifications have already been produced.
    Inline,
    /// Persist the side-effect job in the same transaction as the entity
    /// write; a background worker executes the job at-least-once. Opt-in via
    /// `Prefer: respond-async` plus the configured minimum-batch-size
    /// threshold.
    Enqueue,
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
/// [`EntityService::effective_side_effect_mode`](crate::EntityService::effective_side_effect_mode).
///
/// Split from the method so the decision table can be unit-tested without
/// constructing a full service. See that method's doc comment for the semantic
/// contract.
pub(crate) const fn resolve_side_effect_mode(async_threshold: Option<usize>, preference: AsyncPreference, batch_size: usize) -> BatchSideEffectMode {
    match (preference, async_threshold) {
        (AsyncPreference::Requested, None) => BatchSideEffectMode::Enqueue,
        (AsyncPreference::Requested, Some(threshold)) if batch_size >= threshold => BatchSideEffectMode::Enqueue,
        (AsyncPreference::Requested | AsyncPreference::NotRequested, Some(_)) | (AsyncPreference::NotRequested, None) => BatchSideEffectMode::Inline,
    }
}

#[cfg(test)]
mod tests {
    use super::{AsyncPreference, BatchSideEffectMode, resolve_side_effect_mode};

    #[test]
    fn prefer_respond_async_with_none_threshold_routes_to_enqueue() {
        pretty_assertions::assert_eq!(resolve_side_effect_mode(None, AsyncPreference::Requested, 1), BatchSideEffectMode::Enqueue);
    }

    #[test]
    fn prefer_respond_async_with_threshold_below_batch_size_routes_to_inline() {
        pretty_assertions::assert_eq!(resolve_side_effect_mode(Some(100), AsyncPreference::Requested, 10), BatchSideEffectMode::Inline);
    }

    #[test]
    fn prefer_respond_async_at_or_above_threshold_routes_to_enqueue() {
        pretty_assertions::assert_eq!(
            resolve_side_effect_mode(Some(100), AsyncPreference::Requested, 100),
            BatchSideEffectMode::Enqueue
        );
        pretty_assertions::assert_eq!(
            resolve_side_effect_mode(Some(100), AsyncPreference::Requested, 512),
            BatchSideEffectMode::Enqueue
        );
    }

    #[test]
    fn prefer_respond_sync_forces_inline_with_threshold_zero() {
        pretty_assertions::assert_eq!(
            resolve_side_effect_mode(Some(0), AsyncPreference::NotRequested, 1000),
            BatchSideEffectMode::Inline
        );
    }

    #[test]
    fn no_async_opt_in_routes_to_inline_regardless_of_threshold() {
        pretty_assertions::assert_eq!(resolve_side_effect_mode(None, AsyncPreference::NotRequested, 1000), BatchSideEffectMode::Inline);
        pretty_assertions::assert_eq!(
            resolve_side_effect_mode(Some(1), AsyncPreference::NotRequested, 1000),
            BatchSideEffectMode::Inline
        );
    }
}
