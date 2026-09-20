//! The periodic invalidation pass for one tenant's cached JSON-LD `@contexts`.
//!
//! Clause 5.13.1 and clause 6.3.16 both require it in the same words:
//! "Implementations shall periodically invalidate the "Cached" @contexts."
//! Periodically means on a clock. Reclaiming lapsed rows as a side effect of
//! the next cache write would leave a broker that has stopped downloading new
//! `@contexts` never invalidating the ones it already holds, so the pass runs as
//! its own supervised scheduled task beside the tenant's other lifecycle sweeps.
//!
//! Serving is safe whatever this pass does: the resolver's read already passes
//! over a row whose expiration time has come. What the pass owns is reclamation,
//! and clause 5.13.3's listing telling a client what the broker actually still
//! holds.

use andromeda_context_registry::cached_context_invalidation::CachedContextInvalidation;
use std::time::Duration;

/// How often a tenant's lapsed `"Cached"` Contexts are reclaimed.
///
/// Clause 5.13.1 requires the pass but fixes no cadence, and correctness does
/// not depend on one: an entry stops being served the instant it lapses,
/// whether or not the row has been reclaimed yet. Half a minute keeps the
/// listing close to the truth while costing one narrow `DELETE` over rows of a
/// single kind.
pub(crate) const CACHED_CONTEXT_INVALIDATION_INTERVAL: Duration = Duration::from_secs(30);

/// Run one invalidation pass, reporting what it reclaimed.
///
/// A failed pass is logged and the next tick tries again: the rows it would
/// have reclaimed are already refused by every read, so a database that is
/// briefly unavailable costs storage rather than correctness. The pass never
/// escalates, so the scheduled task that drives it keeps its cadence rather than
/// counting a transient database hiccup against a restart budget.
pub(crate) async fn invalidate_once(store: &dyn CachedContextInvalidation) {
    match store.invalidate_expired().await {
        Ok(invalidated) if invalidated.is_empty() => {}
        Ok(invalidated) => tracing::debug!(reclaimed = invalidated.count(), event = "cached_contexts_invalidated"),
        Err(error) => tracing::warn!(error = %error, event = "cached_context_invalidation_failed"),
    }
}

#[cfg(test)]
mod tests {
    use crate::cached_context_invalidation::invalidate_once;
    use andromeda_context_registry::{
        cached_context_invalidation::{CachedContextInvalidation, CachedContextInvalidationFuture, InvalidatedContexts},
        error::ContextRegistryError,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    /// A store that counts the passes made against it and can be told to fail.
    struct CountingInvalidation {
        /// How many passes have reached the store.
        passes: AtomicU64,
        /// What each pass reports.
        outcome: PassOutcome,
    }

    /// What the fixture's invalidation pass reports back.
    #[derive(Clone, Copy)]
    enum PassOutcome {
        /// The pass reclaimed one Context.
        Reclaimed,
        /// The pass could not reach the store.
        Unavailable,
    }

    impl CachedContextInvalidation for CountingInvalidation {
        fn invalidate_expired(&self) -> CachedContextInvalidationFuture<'_> {
            self.passes.fetch_add(1, Ordering::Relaxed);
            let outcome = self.outcome;
            Box::pin(async move {
                match outcome {
                    PassOutcome::Reclaimed => Ok(InvalidatedContexts::reclaimed(1)),
                    PassOutcome::Unavailable => Err(ContextRegistryError::Persistence {
                        detail: "connection closed".to_owned(),
                    }),
                }
            })
        }
    }

    #[tokio::test]
    async fn a_pass_reaches_the_store() {
        let store = CountingInvalidation {
            passes: AtomicU64::new(0),
            outcome: PassOutcome::Reclaimed,
        };
        invalidate_once(&store).await;
        assert_eq!(store.passes.load(Ordering::Relaxed), 1, "one pass reaches the store");
    }

    #[tokio::test]
    async fn a_failing_pass_is_swallowed_rather_than_escalated() {
        // A pass that cannot reach the store must return normally so the
        // scheduled task keeps its cadence: the rows it would have reclaimed are
        // already refused by every read.
        let store = Arc::new(CountingInvalidation {
            passes: AtomicU64::new(0),
            outcome: PassOutcome::Unavailable,
        });
        invalidate_once(store.as_ref()).await;
        assert_eq!(store.passes.load(Ordering::Relaxed), 1, "the failing pass still reached the store");
    }
}
