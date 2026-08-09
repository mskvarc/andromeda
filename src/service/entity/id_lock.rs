//! Coalescing concurrent writers by entity identifier.
//!
//! Two requests that touch the same entity serialise on a broker-side mutex
//! rather than on an `entities.id` row lock. The difference matters because the
//! guards are taken *before* a pool connection is requested: a caller waiting
//! on a row lock holds a connection for the whole wait, and a burst of
//! overlapping batches would exhaust the pool waiting for itself.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use dashmap::DashMap;
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Map used to coordinate writes by entity id.
pub(crate) type IdLockMap = DashMap<Uri, Arc<Mutex<()>>>;

/// Interval between sweeps of the lock-coalescing map.
///
/// Sized to be roughly two orders of magnitude longer than a typical
/// chunk-write so the dominant cost is the writes themselves, not the sweep.
const ID_LOCK_GC_INTERVAL_SECS: u64 = 60;

/// Acquire mutex guards for `ids` against `map`, in deterministic sorted
/// order. Sorting eliminates lock-ordering deadlocks between concurrent
/// callers whose id sets overlap.
///
/// Callers receive owned guards so the locks remain held for the lifetime
/// of the returned `Vec`. Drop the vec to release every lock at once.
pub(crate) async fn acquire_id_locks_in(map: &IdLockMap, ids: &[Uri]) -> Vec<OwnedMutexGuard<()>> {
    let mut sorted: Vec<&Uri> = ids.iter().collect();
    sorted.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    sorted.dedup_by(|left, right| left.as_str() == right.as_str());

    let mut guards = Vec::with_capacity(sorted.len());
    for id in sorted {
        // The map owns its key independently of the caller's input slice.
        let lock = map.entry(id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).value().clone();
        guards.push(lock.lock_owned().await);
    }
    guards
}

/// Drop entries whose only `Arc` reference is the map itself: nothing else
/// is holding the mutex, so the entry can be reclaimed without losing
/// coordination.
///
/// `DashMap::retain` holds each bucket's lock while inspecting and removing
/// entries from that bucket. Concurrent calls to [`acquire_id_locks_in`]
/// for the same id must take the same bucket lock to insert or read the
/// `Arc<Mutex<()>>`, so they cannot observe a partially-removed state.
pub(crate) fn gc_id_locks(map: &IdLockMap) {
    map.retain(|_, lock| Arc::strong_count(lock) > 1);
}

/// Spawn a background task that periodically prunes unused entries from
/// `weak`. Holding a [`Weak`] instead of a strong [`Arc`] means the task
/// exits as soon as the owning service is dropped — no manual shutdown
/// plumbing required.
pub(crate) fn spawn_id_lock_gc(weak: Weak<IdLockMap>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(ID_LOCK_GC_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // skip the immediate first tick.
        loop {
            ticker.tick().await;
            let Some(map) = weak.upgrade() else {
                break;
            };
            gc_id_locks(&map);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{IdLockMap, acquire_id_locks_in, gc_id_locks};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use std::sync::Arc;

    #[tokio::test]
    async fn id_lock_gc_reclaims_unused_entries_after_guards_drop() {
        let map: Arc<IdLockMap> = Arc::new(IdLockMap::new());
        let ids = [
            "urn:ngsi-ld:Entity:A".parse::<Uri>().expect("valid test URI"),
            "https://example.com/entities/B".parse::<Uri>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:C".parse::<Uri>().expect("valid test URI"),
        ];

        let guards = acquire_id_locks_in(&map, &ids).await;
        pretty_assertions::assert_eq!(map.len(), 3);

        // While guards are alive, GC must keep every entry — strong_count > 1.
        gc_id_locks(&map);
        pretty_assertions::assert_eq!(map.len(), 3);

        drop(guards);
        // Once the only outstanding `Arc` is the map's own, GC reclaims the entry.
        gc_id_locks(&map);
        pretty_assertions::assert_eq!(map.len(), 0);
    }

    #[tokio::test]
    async fn id_lock_acquire_dedupes_and_orders_ids() {
        let map: Arc<IdLockMap> = Arc::new(IdLockMap::new());
        // Duplicates must collapse to a single entry; sorted ordering eliminates
        // deadlocks between concurrent callers with overlapping id sets.
        let ids = [
            "https://example.com/entities/b".parse::<Uri>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:a".parse::<Uri>().expect("valid test URI"),
            "https://example.com/entities/b".parse::<Uri>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:c".parse::<Uri>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:a".parse::<Uri>().expect("valid test URI"),
        ];
        let guards = acquire_id_locks_in(&map, &ids).await;
        pretty_assertions::assert_eq!(guards.len(), 3);
        pretty_assertions::assert_eq!(map.len(), 3);
    }
}
