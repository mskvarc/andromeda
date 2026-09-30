//! Counting the accesses clause 5.13.3.5 reports as `numberOfHits`.
//!
//! Clause 5.13.3.5 permits a `numberOfHits` counter (it is optional) but does
//! not require it to be durable per access, so a resolution bumps an
//! in-process [`crate::hit_counter::HitCounter`] and a background flush folds
//! the deltas into the store; otherwise every expansion would serialise on one
//! `jsonld_contexts` row. Which resolutions count is decided here, beside the
//! pipeline of [`crate::loader::http`] that performs them.

use crate::{
    hit_counter::HitDelta,
    loader::{
        http::HttpContextLoader,
        loaded_context::{ContextDurability, LoadedContext},
        pending_persistence::PersistenceRetryLease,
    },
    operation_hit_scope::{HitAccounting, record_access},
};
use iri_rs::IriBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::warn;

impl HttpContextLoader {
    /// Account for one resolution of `url` that produced `loaded`.
    ///
    /// An entry this caller did not fill was served from the in-process
    /// cache; one it filled from the durable cache is a hit on the stored row;
    /// one it filled by a first download or from the registry is not a hit.
    pub(super) async fn settle_access(&self, url: &IriBuf, loaded: &LoadedContext, filled_here: &AtomicBool) {
        if !filled_here.load(Ordering::Relaxed) {
            self.record_cached_access(url, loaded).await;
        } else if matches!(loaded.durability, ContextDurability::Durable) {
            self.count_access(loaded.canonical_url.as_ref().unwrap_or(url));
        }
    }

    /// Count one context access under clause 5.13.3.5, unless the current
    /// operation scope has already counted this URL.
    ///
    /// Deduplication is per operation ([`crate::operation_hit_scope`]): one HTTP
    /// request or one notification-processing pass that resolves the same
    /// context several times internally counts it once, which is the
    /// client-visible "number of times the @context was found". Outside a scope
    /// every access counts.
    fn count_access(&self, url: &IriBuf) {
        let Some(ref counter) = self.hit_counter else {
            return;
        };
        if matches!(record_access(url), HitAccounting::Count) {
            counter.bump(url);
        }
    }

    /// Record one in-process access and retry pending persistence when necessary.
    ///
    /// Clause 5.13.3.5's `numberOfHits` is the "number of times the @context was
    /// found in the storage", and the in-process cache is the broker's own copy
    /// of that storage rather than a separate place: a request served from it
    /// was served by what the storage holds, so it counts. What the clause does
    /// not settle is how many times *one* request that expands several
    /// documents counts, which is why a single Entity creation shows as several
    /// finds.
    pub(super) async fn record_cached_access(&self, url: &IriBuf, loaded: &LoadedContext) {
        // Clause 5.13.3.5: attribute the access to the URL the stored row keys
        // its `numberOfHits` on, which is the serving URL for a hosted row a
        // request reached by its local id.
        let accounting_url = loaded.canonical_url.as_ref().unwrap_or(url);
        match &loaded.durability {
            ContextDurability::Registry => {}
            ContextDurability::Durable => {
                self.count_access(accounting_url);
            }
            ContextDurability::Pending(pending) if pending.durable.load(Ordering::Acquire) => {
                self.count_access(accounting_url);
            }
            ContextDurability::Pending(pending) => {
                pending.hits.fetch_add(1, Ordering::Relaxed);
                if pending.retrying.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                    return;
                }
                let _lease = PersistenceRetryLease(&pending.retrying);
                let Some(ref cache) = self.l2_cache else {
                    return;
                };
                match cache.put(url, &pending.document_url, &pending.payload, pending.lifetime).await {
                    Ok(()) => {
                        pending.durable.store(true, Ordering::Release);
                        let count = pending.hits.swap(0, Ordering::AcqRel);
                        if count > 0
                            && let Some(ref counter) = self.hit_counter
                        {
                            counter.requeue([HitDelta { url: url.clone(), count }]); // clone: the delta owns its URL while the caller keeps the one it resolved
                        }
                    }
                    Err(error) => warn!(url = %url, error = %error, "Durable context-cache persistence retry failed"),
                }
            }
        }
    }
}
