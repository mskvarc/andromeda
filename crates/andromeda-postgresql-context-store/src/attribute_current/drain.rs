//! Collecting queued events into one apply pass.

use crate::attribute_current::queue::AttributeCurrentReceiver;
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use std::{mem::take, sync::Arc};

/// Hard cap on entries materialised in a single SQL statement.
///
/// Sized so one apply call holds a pool connection for hundreds of
/// milliseconds, not multiple seconds. The previous 50 000 cap let the worker
/// monopolise a connection for 5-9 s on a busy broker, starving user-facing
/// requests. 5 000 keeps the worker's throughput per drain comparable to the
/// in-transaction path it replaced while leaving the rest of the pool free for
/// ingestion.
pub const MAX_DRAIN_BATCH: usize = 5_000;

// A zero cap would make `next_buffered_batch` carry every drained entry
// forward without ever applying one; an unbounded one reintroduces the
// connection starvation described above. Checked here rather than in a unit
// test, where the assertion is a compile-time constant either way.
const _: () = assert!(MAX_DRAIN_BATCH > 0 && MAX_DRAIN_BATCH <= 1_000_000);

/// Entries left over from the previous drain because they exceeded the
/// per-apply cap, together with the sequence number of the event they came
/// from. That sequence may only be published once the leftovers are applied,
/// so the barrier never reports a partially applied batch as complete.
#[derive(Debug, Default)]
pub struct DrainCarry {
    /// Buffered entries awaiting the next apply pass.
    entries: Vec<HistoryBatchEntry>,
    /// Sequence of the partially consumed event; `0` when `entries` is empty.
    seq: u64,
}

/// One drain pass: the entries to apply, plus the highest sequence number that
/// is fully contained in this batch (`0` when none completed).
#[derive(Debug)]
pub struct DrainedBatch {
    /// The history rows to project in this pass.
    pub entries: Vec<HistoryBatchEntry>,
    /// The highest wholly applied sequence, safe to publish as the watermark.
    pub completed_seq: u64,
}

/// Extract the inner `Vec` from an `Arc` without cloning when the caller is the
/// only owner; fall back to `Vec::clone` otherwise.
fn take_or_clone(arc: Arc<Vec<HistoryBatchEntry>>) -> Vec<HistoryBatchEntry> {
    Arc::try_unwrap(arc).unwrap_or_else(|shared| (*shared).clone())
}

/// Drain at most `max` history entries from the channel for one apply pass.
///
/// If the previous call left a tail in `carry`, start with that; otherwise
/// block on the first available event. Subsequent events are drained
/// non-blocking. Any overflow past `max` is stored back into `carry` so the
/// next call applies it first -- no entries are lost.
///
/// Returns `None` only when the channel is closed and no carry remains.
pub async fn next_buffered_batch(rx: &mut AttributeCurrentReceiver, max: usize, carry: &mut DrainCarry) -> Option<DrainedBatch> {
    let mut buffered = take(&mut carry.entries);
    let mut pending_seq = take(&mut carry.seq);
    let mut completed_seq = 0u64;

    if buffered.is_empty() {
        let first = rx.recv().await?;
        pending_seq = first.seq;
        buffered = take_or_clone(first.entries);
    }

    while buffered.len() < max {
        let Ok(next) = rx.try_recv() else {
            break;
        };
        // Everything up to `pending_seq` is now wholly inside `buffered`.
        completed_seq = completed_seq.max(pending_seq);
        pending_seq = next.seq;
        buffered.extend(take_or_clone(next.entries));
    }

    if buffered.len() > max {
        carry.entries = buffered.split_off(max);
        carry.seq = pending_seq;
    } else {
        completed_seq = completed_seq.max(pending_seq);
    }

    Some(DrainedBatch {
        entries: buffered,
        completed_seq,
    })
}

#[cfg(test)]
/// Test utilities for the drain module, shared with sibling test modules.
pub mod tests {
    use super::*;
    use crate::attribute_current::queue::AttributeCurrentEvent;
    use chrono::{TimeZone, Utc};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    /// A history row carrying nothing but its identity, for drain and queue tests.
    pub fn make_entry(tea_id: Uuid) -> HistoryBatchEntry {
        let stamp = Utc.timestamp_opt(1_700_000_000, 0).single().expect("fixed timestamp");
        HistoryBatchEntry {
            time: stamp,
            tea_id,
            value_number: None,
            value_string: None,
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
            instance_id: format!("urn:ngsi-ld:attribute:instance:{tea_id}"),
            observed_at: None,
            metadata: None,
            recorded_at: stamp,
            instance_id_client_supplied: false,
        }
    }

    /// `count` distinct history rows.
    pub fn entries(count: usize) -> Vec<HistoryBatchEntry> {
        (0..count).map(|_| make_entry(Uuid::new_v4())).collect()
    }

    fn event(seq: u64, count: usize) -> AttributeCurrentEvent {
        AttributeCurrentEvent {
            seq,
            entries: Arc::new(entries(count)),
        }
    }

    #[test]
    fn take_or_clone_returns_the_inner_vec_when_unique() {
        let arc = Arc::new(vec![make_entry(Uuid::nil())]);
        pretty_assertions::assert_eq!(take_or_clone(arc).len(), 1);
    }

    #[test]
    fn take_or_clone_clones_when_shared() {
        let arc = Arc::new(vec![make_entry(Uuid::nil())]);
        let _shared = Arc::clone(&arc);
        pretty_assertions::assert_eq!(take_or_clone(arc).len(), 1);
    }

    #[tokio::test]
    async fn a_single_event_drains_whole_and_then_the_closed_channel_ends_the_loop() {
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        tx.send(event(1, 3)).await.expect("send");
        drop(tx);

        let mut carry = DrainCarry::default();
        let batch = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await.expect("batch");
        pretty_assertions::assert_eq!(batch.entries.len(), 3);
        pretty_assertions::assert_eq!(batch.completed_seq, 1);
        assert!(carry.entries.is_empty());

        assert!(
            next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await.is_none(),
            "channel closed and carry empty must end the loop"
        );
    }

    #[tokio::test]
    async fn several_buffered_events_drain_in_one_pass() {
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        for seq in 1..=5 {
            tx.send(event(seq, 2)).await.expect("send");
        }
        drop(tx);

        let mut carry = DrainCarry::default();
        let batch = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await.expect("batch");
        pretty_assertions::assert_eq!(batch.entries.len(), 10, "five 2-entry events drained as one 10-entry batch");
        pretty_assertions::assert_eq!(batch.completed_seq, 5, "the watermark covers every event in the batch");
        assert!(carry.entries.is_empty());
    }

    #[tokio::test]
    async fn every_drained_batch_respects_the_cap_and_nothing_is_lost() {
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        // 4 events of 30 entries each = 120 entries with a cap of 50.
        for seq in 1..=4 {
            tx.send(event(seq, 30)).await.expect("send");
        }
        drop(tx);

        let mut carry = DrainCarry::default();
        let mut applied: Vec<usize> = Vec::new();
        while let Some(batch) = next_buffered_batch(&mut rx, 50, &mut carry).await {
            applied.push(batch.entries.len());
        }
        assert!(!applied.is_empty(), "at least one batch must be applied");
        for size in &applied {
            assert!(*size <= 50, "every batch must respect the cap; got {}", size);
        }
        pretty_assertions::assert_eq!(applied.iter().sum::<usize>(), 120, "all entries must be flushed across batches");
    }

    #[tokio::test]
    async fn the_watermark_never_reports_a_partially_applied_event() {
        // Two 30-entry events with a cap of 50: the first batch takes all of
        // event 1 plus 20 rows of event 2, so only seq 1 may be published.
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        tx.send(event(1, 30)).await.expect("send");
        tx.send(event(2, 30)).await.expect("send");
        drop(tx);

        let mut carry = DrainCarry::default();
        let first = next_buffered_batch(&mut rx, 50, &mut carry).await.expect("batch");
        pretty_assertions::assert_eq!(first.entries.len(), 50);
        pretty_assertions::assert_eq!(first.completed_seq, 1, "event 2 is only half applied and must not be published");
        pretty_assertions::assert_eq!(carry.entries.len(), 10);
        pretty_assertions::assert_eq!(carry.seq, 2);

        let second = next_buffered_batch(&mut rx, 50, &mut carry).await.expect("batch");
        pretty_assertions::assert_eq!(second.entries.len(), 10);
        pretty_assertions::assert_eq!(second.completed_seq, 2, "event 2 completes once its tail is applied");
    }
}
