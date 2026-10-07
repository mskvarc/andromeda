//! Coalescing concurrent writers by entity identifier.
//!
//! Two requests that touch the same entity serialise on a broker-side mutex
//! rather than on an `entities.id` row lock. The difference matters because the
//! guards are taken *before* a pool connection is requested: a caller waiting
//! on a row lock holds a connection for the whole wait, and a burst of
//! overlapping batches would exhaust the pool waiting for itself.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_lock::{Mutex, MutexGuardArc};
use dashmap::DashMap;
use std::sync::Arc;

/// Map used to coordinate writes by entity id.
pub(crate) type IdLockMap = DashMap<IdLockKey, Arc<Mutex<()>>>;

/// Identity of one mutable entity inside one context branch.
///
/// The branch is the live branch (`0`) or a Snapshot branch (its positive
/// `snapshot_key`), so the two never collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct IdLockKey {
    branch_key: i64,
    entity_id: Identifier,
}

/// Acquire mutex guards for `ids` against `map`, in deterministic sorted
/// order. Sorting eliminates lock-ordering deadlocks between concurrent
/// callers whose id sets overlap.
///
/// Callers receive owned guards so the locks remain held for the lifetime
/// of the returned `Vec`. Drop the vec to release every lock at once.
pub(crate) async fn acquire_id_locks_in(map: &IdLockMap, target: &ContextTarget, ids: &[Identifier]) -> Vec<MutexGuardArc<()>> {
    gc_id_locks(map);
    let mut sorted: Vec<&Identifier> = ids.iter().collect();
    sorted.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    sorted.dedup_by(|left, right| left.as_str() == right.as_str());

    let mut guards = Vec::with_capacity(sorted.len());
    for id in sorted {
        // The map owns its key independently of the caller's input slice.
        let key = IdLockKey {
            branch_key: target.snapshot().map_or(0_i64, |snapshot| snapshot.key().as_i64()),
            entity_id: id.clone(), // clone: the lock map owns the identifier after the request ends.
        };
        let lock = map.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))).value().clone();
        guards.push(lock.lock_arc().await);
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

#[cfg(test)]
mod tests {
    use super::{IdLockMap, acquire_id_locks_in, gc_id_locks};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::sync::Arc;

    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    #[tokio::test]
    async fn id_lock_gc_reclaims_unused_entries_after_guards_drop() {
        let map: Arc<IdLockMap> = Arc::new(IdLockMap::new());
        let ids = [
            "urn:ngsi-ld:Entity:A".parse::<Identifier>().expect("valid test URI"),
            "https://example.com/entities/B".parse::<Identifier>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:C".parse::<Identifier>().expect("valid test URI"),
        ];

        let guards = acquire_id_locks_in(&map, &live_target(), &ids).await;
        pretty_assertions::assert_eq!(map.len(), 3);

        gc_id_locks(&map);
        pretty_assertions::assert_eq!(map.len(), 3);

        drop(guards);
        gc_id_locks(&map);
        pretty_assertions::assert_eq!(map.len(), 0);
    }

    #[tokio::test]
    async fn id_lock_acquire_dedupes_and_orders_ids() {
        let map: Arc<IdLockMap> = Arc::new(IdLockMap::new());
        let ids = [
            "https://example.com/entities/b".parse::<Identifier>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:a".parse::<Identifier>().expect("valid test URI"),
            "https://example.com/entities/b".parse::<Identifier>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:c".parse::<Identifier>().expect("valid test URI"),
            "urn:ngsi-ld:Entity:a".parse::<Identifier>().expect("valid test URI"),
        ];
        let guards = acquire_id_locks_in(&map, &live_target(), &ids).await;
        pretty_assertions::assert_eq!(guards.len(), 3);
        pretty_assertions::assert_eq!(map.len(), 3);
    }
}
