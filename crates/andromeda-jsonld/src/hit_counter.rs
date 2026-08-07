//! Clause 5.13.3.5 access counting, off the expansion path.
//!
//! The clause requires a broker to maintain a `numberOfHits` per hosted
//! context; it does not require the count to be durable at the moment of
//! access. Every expansion bumping a shared row would serialise the whole
//! ingestion path on it, so hits accumulate here as per-URL atomics and a
//! background task folds each cycle's deltas into one bulk update.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-URL access counts awaiting a flush.
#[derive(Debug, Default)]
pub struct HitCounter {
    /// Counts accumulated since the last drain, one atomic per URL.
    pending: DashMap<String, AtomicU64>,
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
    pub fn bump(&self, url: &str) {
        if let Some(entry) = self.pending.get(url) {
            entry.value().fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.pending
            .entry(url.to_string()) // clone: DashMap takes an owned key on insert
            .or_insert_with(|| AtomicU64::new(0))
            .value()
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Remove every pending count and answer with the drained deltas.
    pub fn drain(&self) -> Vec<(String, u64)> {
        let keys: Vec<_> = self.pending.iter().map(|entry| entry.key().clone()).collect();
        let mut drained = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some((url, count)) = self.pending.remove(&key) {
                let delta = count.into_inner();
                if delta > 0 {
                    drained.push((url, delta));
                }
            }
        }
        drained
    }
}

#[cfg(test)]
mod tests {
    use crate::hit_counter::HitCounter;

    #[test]
    fn counts_accumulate_per_url_and_a_drain_removes_their_entries() {
        let counter = HitCounter::new();
        let url_a = "https://example.com/ctx-a";
        let url_b = "https://example.com/ctx-b";

        for _ in 0..5 {
            counter.bump(url_a);
        }
        counter.bump(url_b);
        counter.bump(url_b);

        let mut drained = counter.drain();
        drained.sort_by(|left, right| left.0.cmp(&right.0));
        pretty_assertions::assert_eq!(drained, vec![(url_a.to_string(), 5), (url_b.to_string(), 2)]);

        let second = counter.drain();
        assert!(second.is_empty(), "a drain of zero deltas reports nothing, got {second:?}");

        counter.bump(url_a);
        pretty_assertions::assert_eq!(counter.drain(), vec![(url_a.to_string(), 1)]);
    }
}
