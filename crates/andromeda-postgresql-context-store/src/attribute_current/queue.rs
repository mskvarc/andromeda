//! The queue between committers and the materializer, and its two handles.
//!
//! The producer and the barrier are one protocol seen from two ends: the
//! producer stamps every published batch with a monotonic sequence number, and
//! the barrier compares that high-water mark against the worker's applied
//! watermark. They share the counter and change together.

use andromeda_postgresql_query::{history_batch_entry::HistoryBatchEntry, projection_barrier::ProjectionBarrier};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{mpsc, watch};
use tracing::{error, warn};

/// Depth of the materializer queue, in batches.
///
/// The queue is bounded so a materializer that falls behind applies
/// backpressure to committers instead of growing without limit. Enqueueing
/// happens *after* the database commit, so a full queue never holds a
/// transaction open -- it only delays the producer's return. Sized to absorb a
/// multi-second stall at batch-ingest rates while keeping the worst-case
/// resident set bounded.
const QUEUE_CAPACITY: usize = 1_024;

/// A batch of history entries queued for projection into `attribute_current`,
/// tagged with its position in the issue order.
///
/// The `Arc` wrapper avoids cloning the producer's vector when the channel
/// queue is shallow: with a single consumer and no other outstanding
/// references, `Arc::try_unwrap` succeeds.
#[derive(Debug)]
pub struct AttributeCurrentEvent {
    /// Monotonic issue sequence, claimed by [`AttributeCurrentQueue::reserve`].
    pub(super) seq: u64,
    /// History rows to project.
    pub(super) entries: Arc<Vec<HistoryBatchEntry>>,
}

/// Consumer-side channel handle owned by the worker.
pub(super) type AttributeCurrentReceiver = mpsc::Receiver<AttributeCurrentEvent>;

/// Producer handle used by the write path's commit hook.
///
/// Assigns each batch a monotonic sequence number, which is what lets
/// [`AttributeCurrentBarrier`] express "everything issued before now" without
/// putting a message on the queue.
#[derive(Debug, Clone)]
pub struct AttributeCurrentQueue {
    /// Channel sender to the materializer worker.
    tx: mpsc::Sender<AttributeCurrentEvent>,
    /// Monotonic issue counter shared with the barrier.
    issued: Arc<AtomicU64>,
}

impl AttributeCurrentQueue {
    /// Claim a sequence number for `entries` without enqueueing them yet.
    ///
    /// Returns `None` for an empty batch, which consumes no sequence and would
    /// occupy a queue slot for no reason.
    ///
    /// The claim is separate from the enqueue because the two must straddle the
    /// database commit. Feeding the projection *before* the commit would answer
    /// a `q` filter with a value no reader can see; claiming only *after* it
    /// leaves a window in which the entity row is visible, the high-water mark
    /// has not moved, and a concurrent query's barrier therefore reports nothing
    /// pending and reads a projection that has not seen the write. Claiming
    /// before and enqueueing after closes it.
    pub fn reserve(&self, entries: Vec<HistoryBatchEntry>) -> Option<ReservedPublish> {
        if entries.is_empty() {
            return None;
        }
        let seq = self.issued.fetch_add(1, Ordering::AcqRel) + 1;
        Some(ReservedPublish {
            tx: self.tx.clone(), // clone: an mpsc `Sender` clone is a ref-count bump
            seq,
            entries,
        })
    }
}

/// A sequence number claimed for a batch that has not been enqueued yet.
///
/// Every reservation must be resolved: [`ReservedPublish::enqueue`] when the
/// commit succeeded, [`ReservedPublish::abandon`] when it did not. One that is
/// neither leaves [`AttributeCurrentBarrier`] waiting on a sequence no batch
/// will ever carry, which is why this type is `#[must_use]` and why the abandon
/// path exists at all.
#[derive(Debug)]
#[must_use = "a reservation that is neither enqueued nor abandoned stalls the read-your-writes barrier"]
pub struct ReservedPublish {
    /// Channel sender to the materializer worker.
    tx: mpsc::Sender<AttributeCurrentEvent>,
    /// Claimed sequence number.
    seq: u64,
    /// History rows to project.
    entries: Vec<HistoryBatchEntry>,
}

impl ReservedPublish {
    /// Hand the reserved batch to the materializer.
    ///
    /// Awaits queue capacity when the materializer is behind. A closed channel
    /// (worker exited) is logged and swallowed: `attribute_history` is the
    /// canonical source and the startup reconcile refills the projection.
    pub async fn enqueue(self) {
        let ReservedPublish { tx, seq, entries } = self;
        send(&tx, seq, entries).await;
    }

    /// Release the reservation without projecting anything.
    ///
    /// Sends the claimed sequence carrying no rows, so the materializer advances
    /// its watermark past it and a barrier already waiting on that sequence is
    /// released. Dropping the entries instead would stall every such waiter for
    /// the life of the process.
    pub async fn abandon(self) {
        let ReservedPublish { tx, seq, entries } = self;
        drop(entries);
        send(&tx, seq, Vec::new()).await;
    }
}

/// Enqueue one event, logging a closed channel rather than failing the caller.
async fn send(tx: &mpsc::Sender<AttributeCurrentEvent>, seq: u64, entries: Vec<HistoryBatchEntry>) {
    let event = AttributeCurrentEvent {
        seq,
        entries: Arc::new(entries),
    };
    if tx.send(event).await.is_err() {
        warn!(event = "attribute_current_publish_failed");
    }
}

/// Read-your-writes barrier for the query path.
///
/// Held by the services that compile `q` filters against `attribute_current`.
/// [`AttributeCurrentBarrier::wait_for_pending_writes`] samples the current
/// issue high-water mark and waits until the materializer's applied watermark
/// reaches it, so a query issued after a committed write observes that write.
///
/// This is a watermark rather than a queued barrier message deliberately: a
/// barrier message would sit in the same FIFO as the batches, forcing a drain
/// boundary at every `q`-filtered query and coupling read latency to write
/// backlog. Sampling a watermark adds no queue traffic, does not fragment the
/// worker's batching, and lets concurrent waiters share one notification.
#[derive(Debug, Clone)]
pub struct AttributeCurrentBarrier {
    /// High-water mark: the highest sequence any committer has claimed.
    issued: Arc<AtomicU64>,
    /// Receiver for the worker's applied watermark.
    applied: watch::Receiver<u64>,
}

impl AttributeCurrentBarrier {
    /// Wait until every batch queued before this call has been applied.
    ///
    /// A dead worker (closed watch channel) is logged and then ignored:
    /// availability of the read path is preferred over blocking it on a
    /// background worker.
    pub async fn wait_for_pending_writes(&self) {
        let target = self.issued.load(Ordering::Acquire);
        let mut applied = self.applied.clone(); // clone: per-call cursor; watch::Receiver clone is cheap
        if *applied.borrow_and_update() >= target {
            return;
        }
        while applied.changed().await.is_ok() {
            if *applied.borrow_and_update() >= target {
                return;
            }
        }
        error!(target_seq = target, event = "attribute_current_barrier_worker_gone");
    }
}

#[async_trait::async_trait]
impl ProjectionBarrier for AttributeCurrentBarrier {
    async fn wait_for_pending_writes(&self) {
        AttributeCurrentBarrier::wait_for_pending_writes(self).await;
    }
}

/// Open the queue, returning the worker's receiver and the two producer-side handles.
///
/// `applied` is the worker's watermark receiver; the barrier waits on it.
pub(super) fn open(applied: watch::Receiver<u64>) -> (AttributeCurrentReceiver, AttributeCurrentQueue, AttributeCurrentBarrier) {
    let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
    let issued = Arc::new(AtomicU64::new(0));
    let queue = AttributeCurrentQueue {
        tx,
        issued: Arc::clone(&issued), // clone: Arc ref-count bump; the barrier reads the same counter
    };
    let barrier = AttributeCurrentBarrier { issued, applied };
    (rx, queue, barrier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute_current::drain::tests::entries;

    fn opened() -> (watch::Sender<u64>, AttributeCurrentReceiver, AttributeCurrentQueue, AttributeCurrentBarrier) {
        let (applied_tx, applied_rx) = watch::channel(0u64);
        let (rx, queue, barrier) = open(applied_rx);
        (applied_tx, rx, queue, barrier)
    }

    #[tokio::test]
    async fn reserving_assigns_monotonic_sequences() {
        let (_applied_tx, mut rx, queue, _barrier) = opened();
        queue.reserve(entries(1)).expect("first reservation").enqueue().await;
        queue.reserve(entries(1)).expect("second reservation").enqueue().await;
        pretty_assertions::assert_eq!(rx.try_recv().expect("first").seq, 1);
        pretty_assertions::assert_eq!(rx.try_recv().expect("second").seq, 2);
    }

    #[tokio::test]
    async fn reserving_ignores_empty_batches() {
        let (_applied_tx, mut rx, queue, barrier) = opened();
        assert!(queue.reserve(Vec::new()).is_none(), "an empty batch claims no sequence");
        assert!(rx.try_recv().is_err(), "empty batch must not occupy a queue slot");
        pretty_assertions::assert_eq!(barrier.issued.load(Ordering::Acquire), 0, "empty batch must not advance the high-water mark");
    }

    #[tokio::test]
    async fn a_reservation_advances_the_high_water_mark_before_it_is_enqueued() {
        // This is the whole point of separating the two: the committer claims
        // the sequence, commits, and only then enqueues. A query that samples
        // the mark inside that window must see a pending batch and wait.
        let (_applied_tx, mut rx, queue, barrier) = opened();
        let reserved = queue.reserve(entries(1)).expect("reservation");

        pretty_assertions::assert_eq!(barrier.issued.load(Ordering::Acquire), 1, "the mark moves at reservation, not at enqueue");
        assert!(rx.try_recv().is_err(), "nothing is on the queue yet");

        reserved.enqueue().await;
        pretty_assertions::assert_eq!(rx.try_recv().expect("enqueued").seq, 1);
    }

    #[tokio::test]
    async fn an_abandoned_reservation_still_releases_the_barrier() {
        // A commit that failed must not strand a waiter on a sequence no batch
        // will ever carry, so the claimed sequence is sent carrying no rows.
        let (_applied_tx, mut rx, queue, barrier) = opened();
        queue.reserve(entries(3)).expect("reservation").abandon().await;

        pretty_assertions::assert_eq!(barrier.issued.load(Ordering::Acquire), 1);
        let event = rx.try_recv().expect("abandoned reservations still reach the worker");
        pretty_assertions::assert_eq!(event.seq, 1);
        assert!(event.entries.is_empty(), "an abandoned batch projects nothing");
    }

    #[tokio::test]
    async fn the_barrier_returns_once_the_watermark_reaches_the_issue_high_water_mark() {
        let (applied_tx, _rx, queue, barrier) = opened();
        queue.reserve(entries(1)).expect("first reservation").enqueue().await;
        queue.reserve(entries(1)).expect("second reservation").enqueue().await;

        // Nothing applied yet: the barrier must not be satisfied.
        pretty_assertions::assert_eq!(*barrier.applied.borrow(), 0);
        pretty_assertions::assert_eq!(barrier.issued.load(Ordering::Acquire), 2);

        // Publishing the watermark for both events releases the barrier.
        applied_tx.send(2).expect("watermark send");
        barrier.wait_for_pending_writes().await;
    }

    #[tokio::test]
    async fn the_barrier_is_satisfied_immediately_when_nothing_is_queued() {
        let (_applied_tx, _rx, _queue, barrier) = opened();
        barrier.wait_for_pending_writes().await;
    }
}
