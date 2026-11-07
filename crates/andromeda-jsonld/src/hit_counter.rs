//! Clause 5.13.3.5 access counting, off the expansion path.
//!
//! The clause permits a broker to maintain a `numberOfHits` per hosted context
//! (`numberOfHits` is optional per clause 5.13.3.5); it does not require the
//! count to be durable at the moment of
//! access. Every expansion bumping a shared row would serialise the whole
//! ingestion path on it, so hits accumulate here as per-URL atomics and a
//! background task folds each cycle's deltas into one bulk update.
//!
//! A reader reports the stored count plus the pending one. A flush drains the
//! pending counts before the stored row takes them, so a reader between the
//! two would see neither; the flush therefore holds the counter's gate
//! exclusively from the drain until the row has taken the deltas or they are
//! back in the counter, and a reader holds it shared from reading the row until
//! it has added the pending count.

use dashmap::DashMap;
use iri_rs::IriBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// One durable context access delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitDelta {
    /// The cached context URL whose row receives the delta.
    pub url: IriBuf,
    /// The number of accesses accumulated for the URL.
    pub count: u64,
}

/// Per-URL access counts awaiting a flush.
#[derive(Debug, Default)]
pub struct HitCounter {
    /// Counts accumulated since the last drain, one atomic per URL.
    pending: DashMap<IriBuf, AtomicU64>,
    /// Keeps a reader's stored-plus-pending sum apart from a flush moving
    /// counts from one to the other.
    gate: RwLock<()>,
}

impl HitCounter {
    /// An empty counter.
    #[must_use]
    pub fn new() -> HitCounter {
        HitCounter::default()
    }

    /// Record one access of a context URL.
    ///
    /// Steady state is a map read and an atomic add; only the first access of
    /// a URL takes the insert path.
    pub fn bump(&self, url: &IriBuf) {
        if let Some(entry) = self.pending.get(url) {
            entry.value().fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.pending
            .entry(url.clone()) // clone: DashMap takes an owned key on insert
            .or_insert_with(|| AtomicU64::new(0))
            .value()
            .fetch_add(1, Ordering::Relaxed);
    }

    /// The count accumulated for a URL since the last drain.
    ///
    /// A reader of clause 5.13.3.5's `numberOfHits` adds this to the stored
    /// count, so the number it reports is current rather than as of the last
    /// flush.
    #[must_use]
    pub fn pending(&self, url: &IriBuf) -> u64 {
        self.pending.get(url).map_or(0, |entry| entry.value().load(Ordering::Relaxed))
    }

    /// Hold flushes off while the stored count of a URL is read and its
    /// pending count added to it, so the sum counts every access once.
    pub async fn read_settled(&self) -> RwLockReadGuard<'_, ()> {
        self.gate.read().await
    }

    /// Hold readers off while drained deltas move into the stored rows, so no
    /// reader sees them in neither place; held until the store has taken them
    /// or they were requeued.
    pub async fn flush_exclusively(&self) -> RwLockWriteGuard<'_, ()> {
        self.gate.write().await
    }

    /// Remove every pending count and answer with the drained deltas.
    #[must_use]
    pub fn drain(&self) -> Vec<HitDelta> {
        let keys: Vec<_> = self.pending.iter().map(|entry| entry.key().clone()).collect();
        let mut drained = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some((url, count)) = self.pending.remove(&key) {
                let delta = count.into_inner();
                if delta > 0 {
                    drained.push(HitDelta { url, count: delta });
                }
            }
        }
        drained
    }

    /// Put a failed flush back into the pending set without losing concurrent bumps.
    pub fn requeue(&self, deltas: impl IntoIterator<Item = HitDelta>) {
        for delta in deltas {
            self.pending
                .entry(delta.url)
                .or_insert_with(|| AtomicU64::new(0))
                .value()
                .fetch_add(delta.count, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::hit_counter::{HitCounter, HitDelta};
    use iri_rs::IriBuf;
    use std::time::Duration;
    use tokio::time::timeout;

    /// How long a blocked acquisition is watched before it counts as blocked.
    const BLOCKED: Duration = Duration::from_millis(50);

    /// A reader adding the pending count to a stored one and a flush moving
    /// counts from one to the other exclude each other, so no reader sees a
    /// drained delta in neither place.
    #[tokio::test]
    async fn a_reader_and_a_flush_exclude_each_other() {
        let counter = HitCounter::new();

        let reading = counter.read_settled().await;
        assert!(timeout(BLOCKED, counter.flush_exclusively()).await.is_err(), "a flush waits for the reader");
        assert!(timeout(BLOCKED, counter.read_settled()).await.is_ok(), "readers share the gate");
        drop(reading);

        let flushing = counter.flush_exclusively().await;
        assert!(timeout(BLOCKED, counter.read_settled()).await.is_err(), "a reader waits for the flush");
        drop(flushing);
        assert!(
            timeout(BLOCKED, counter.read_settled()).await.is_ok(),
            "the reader proceeds once the flush is done"
        );
    }

    #[test]
    fn counts_accumulate_per_url_and_a_drain_removes_their_entries() {
        let counter = HitCounter::new();
        let url_a = IriBuf::new("https://example.com/ctx-a".to_string()).expect("valid URL");
        let url_b = IriBuf::new("https://example.com/ctx-b".to_string()).expect("valid URL");

        for _ in 0..5 {
            counter.bump(&url_a);
        }
        counter.bump(&url_b);
        counter.bump(&url_b);

        let mut drained = counter.drain();
        drained.sort_by(|left, right| left.url.cmp(&right.url));
        pretty_assertions::assert_eq!(drained, vec![HitDelta { url: url_a.clone(), count: 5 }, HitDelta { url: url_b, count: 2 },]);

        let second = counter.drain();
        assert!(second.is_empty(), "a drain of zero deltas reports nothing, got {second:?}");

        counter.bump(&url_a);
        pretty_assertions::assert_eq!(counter.drain(), vec![HitDelta { url: url_a, count: 1 }]);
    }

    #[test]
    fn a_failed_batch_can_be_requeued_without_losing_new_hits() {
        let counter = HitCounter::new();
        let url = IriBuf::new("https://example.com/ctx".to_string()).expect("valid URL");

        counter.bump(&url);
        let pending = counter.drain();
        counter.bump(&url);
        counter.requeue(pending);

        pretty_assertions::assert_eq!(counter.drain(), vec![HitDelta { url, count: 2 }]);
    }
}
