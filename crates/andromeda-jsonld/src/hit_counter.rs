//! Clause 5.13.3.5 access counting, off the expansion path.
//!
//! The clause requires a broker to maintain a `numberOfHits` per hosted
//! context; it does not require the count to be durable at the moment of
//! access. Every expansion bumping a shared row would serialise the whole
//! ingestion path on it, so hits accumulate here as per-URL atomics and a
//! background task folds each cycle's deltas into one bulk update.

use dashmap::DashMap;
use iri_rs::IriBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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
}

impl HitCounter {
    /// An empty counter.
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

    /// Remove every pending count and answer with the drained deltas.
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
    use crate::hit_counter::HitCounter;
    use iri_rs::IriBuf;

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
        pretty_assertions::assert_eq!(
            drained,
            vec![
                crate::hit_counter::HitDelta { url: url_a.clone(), count: 5 },
                crate::hit_counter::HitDelta { url: url_b, count: 2 },
            ]
        );

        let second = counter.drain();
        assert!(second.is_empty(), "a drain of zero deltas reports nothing, got {second:?}");

        counter.bump(&url_a);
        pretty_assertions::assert_eq!(counter.drain(), vec![crate::hit_counter::HitDelta { url: url_a, count: 1 }]);
    }

    #[test]
    fn a_failed_batch_can_be_requeued_without_losing_new_hits() {
        let counter = HitCounter::new();
        let url = IriBuf::new("https://example.com/ctx".to_string()).expect("valid URL");

        counter.bump(&url);
        let pending = counter.drain();
        counter.bump(&url);
        counter.requeue(pending);

        pretty_assertions::assert_eq!(counter.drain(), vec![crate::hit_counter::HitDelta { url, count: 2 }]);
    }
}
