//! Bounded concurrent execution of work that runs at most once per key at a
//! time, detached from whoever offered it.
//!
//! Offering work returns at once: the work waits for one of a fixed number of
//! slots, runs, and frees its key. An offer for a key whose work is still
//! queued or running is refused, and the refusal is remembered, so the running
//! work's `freed` callback tells its owner once the key is free and the work
//! can be offered anew. Cancelling the slots stops every queued and running
//! work item at its next await point.

use dashmap::{DashMap, mapref::entry::Entry};
use std::{hash::Hash, num::NonZeroUsize, sync::Arc};
use tokio::sync::Semaphore;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::warn;

/// Whether an offered work item was taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    /// The work was handed to a task and waits for a slot or runs.
    Started,
    /// Work of the same key is still queued or running, so this one was
    /// dropped; the running work's `freed` callback runs once the key is free.
    Busy,
}

/// Whether an offer for a key was refused while its work was in flight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Contention {
    /// No offer for the key was refused.
    Uncontended,
    /// An offer for the key was refused, so its owner is told when it frees.
    Refused,
}

/// A fixed number of slots shared by work items keyed by `K`.
///
/// Cloning yields another handle to the same slots.
#[derive(Clone, Debug)]
pub struct KeyedSlots<K: Eq + Hash> {
    /// The state every handle and every spawned work item shares.
    inner: Arc<SlotsInner<K>>,
}

/// The state behind [`KeyedSlots`].
#[derive(Debug)]
struct SlotsInner<K: Eq + Hash> {
    /// The keys whose work is queued or running, with whether an offer for
    /// them was refused meanwhile.
    in_flight: DashMap<K, Contention>,
    /// One permit per slot; a work item holds one while it runs.
    slots: Semaphore,
    /// The spawned work items, awaited by [`KeyedSlots::finish`].
    tasks: TaskTracker,
    /// Stops every queued and running work item.
    cancellation: CancellationToken,
}

impl<K: Eq + Hash + Copy + Send + Sync + 'static> KeyedSlots<K> {
    /// `width` slots, whose work stops when `cancellation` is cancelled.
    #[must_use]
    pub fn new(width: NonZeroUsize, cancellation: CancellationToken) -> KeyedSlots<K> {
        KeyedSlots {
            inner: Arc::new(SlotsInner {
                in_flight: DashMap::new(),
                slots: Semaphore::new(width.get()),
                tasks: TaskTracker::new(),
                cancellation,
            }),
        }
    }

    /// Hand `work` for `key` to a task of its own, unless work of `key` is
    /// still in flight.
    ///
    /// `freed` runs once `work` has ended, after `key` is free again, but only
    /// when an offer for `key` was refused while `work` was in flight. It is
    /// not run when `work` is cancelled.
    ///
    /// The task is spawned on the Tokio runtime the caller runs on.
    pub fn offer<W, F>(&self, key: K, work: W, freed: F) -> Admission
    where
        W: Future<Output = ()> + Send + 'static,
        F: FnOnce() + Send + 'static,
    {
        match self.inner.in_flight.entry(key) {
            Entry::Occupied(mut running) => {
                *running.get_mut() = Contention::Refused;
                return Admission::Busy;
            }
            Entry::Vacant(free) => {
                free.insert(Contention::Uncontended);
            }
        }
        let inner = Arc::clone(&self.inner); // clone: the spawned work frees its key in the shared map.
        self.inner.tasks.spawn(async move {
            let release = Release {
                inner: &inner,
                key,
                freed: Some(freed),
            };
            tokio::select! {
                () = inner.cancellation.cancelled() => release.cancelled(),
                () = run_in_slot(&inner.slots, work) => drop(release),
            }
        });
        Admission::Started
    }

    /// Stop every queued and running work item and wait until each has ended.
    pub async fn finish(&self) {
        self.inner.tasks.close();
        self.inner.cancellation.cancel();
        self.inner.tasks.wait().await;
    }

    /// Whether work of `key` is queued or running.
    #[must_use]
    pub fn is_in_flight(&self, key: &K) -> bool {
        self.inner.in_flight.contains_key(key)
    }
}

/// Run `work` once a slot is free, holding the slot until it ends.
async fn run_in_slot<W: Future<Output = ()>>(slots: &Semaphore, work: W) {
    match slots.acquire().await {
        Ok(_slot) => work.await,
        // The semaphore is never closed, so a refused acquisition means the
        // slots were torn down; the work is dropped with them.
        Err(error) => warn!(%error, "a keyed slot could not be acquired"),
    }
}

/// Frees one work item's key however its task ends: run out, cancelled, or
/// unwound.
struct Release<'a, K: Eq + Hash, F: FnOnce()> {
    /// The slots the key is freed in.
    inner: &'a SlotsInner<K>,
    /// The key the work item holds.
    key: K,
    /// The owner's callback, taken when the work was cancelled.
    freed: Option<F>,
}

impl<K: Eq + Hash, F: FnOnce()> Release<'_, K, F> {
    /// Free the key without telling the owner: the slots are shutting down.
    fn cancelled(mut self) {
        self.freed = None;
    }
}

impl<K: Eq + Hash, F: FnOnce()> Drop for Release<'_, K, F> {
    // Freeing the key is cleanup that must run however the task ends, which
    // no derive expresses.
    fn drop(&mut self) {
        let refused = self
            .inner
            .in_flight
            .remove(&self.key)
            .is_some_and(|(_, contention)| contention == Contention::Refused);
        if refused && let Some(freed) = self.freed.take() {
            freed();
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::keyed_slots::{Admission, KeyedSlots};
    use std::{future, num::NonZeroUsize, time::Duration};
    use tokio::{
        sync::{mpsc, oneshot},
        task::yield_now,
        time::{sleep, timeout},
    };
    use tokio_util::sync::CancellationToken;

    fn slots(width: usize) -> KeyedSlots<u32> {
        KeyedSlots::new(NonZeroUsize::new(width).expect("a positive width"), CancellationToken::new())
    }

    async fn within<T>(what: impl Future<Output = T>) -> T {
        timeout(Duration::from_secs(5), what).await.expect("done within the timeout")
    }

    #[tokio::test]
    async fn work_offered_for_a_key_in_flight_is_refused_and_its_owner_is_told_when_the_key_frees() {
        let slots = slots(4);
        let (open, gate) = oneshot::channel::<()>();
        let (freed_tx, mut freed) = mpsc::unbounded_channel();
        let first = slots.offer(
            1,
            async move {
                gate.await.expect("the test opens the gate");
            },
            move || freed_tx.send(()).expect("the test reads the callbacks"),
        );
        pretty_assertions::assert_eq!(first, Admission::Started);
        pretty_assertions::assert_eq!(slots.offer(1, async {}, || {}), Admission::Busy);

        open.send(()).expect("the work waits on its gate");
        within(freed.recv()).await.expect("the owner is told the key freed");
        assert!(!slots.is_in_flight(&1), "the key is free before its owner is told");
        pretty_assertions::assert_eq!(slots.offer(1, async {}, || {}), Admission::Started);
    }

    #[tokio::test]
    async fn work_whose_key_was_never_contended_frees_it_without_telling_its_owner() {
        let slots = slots(4);
        let (done_tx, done) = oneshot::channel();
        let (freed_tx, mut freed) = mpsc::unbounded_channel::<()>();
        slots.offer(
            1,
            async move {
                done_tx.send(()).expect("the test waits for the work");
            },
            move || freed_tx.send(()).expect("the test reads the callbacks"),
        );
        within(done).await.expect("the work ran");
        within(async {
            while slots.is_in_flight(&1) {
                yield_now().await;
            }
        })
        .await;
        assert!(freed.try_recv().is_err(), "nobody asked for the key while it was held");
    }

    #[tokio::test]
    async fn work_beyond_the_width_waits_for_a_free_slot_while_its_offer_returns_at_once() {
        let slots = slots(1);
        let (open, gate) = oneshot::channel::<()>();
        let (ran_tx, mut ran) = mpsc::unbounded_channel();
        let blocking = ran_tx.clone(); // clone: both work items log through one channel
        slots.offer(
            1,
            async move {
                gate.await.expect("the test opens the gate");
                blocking.send(1).expect("the test reads the log");
            },
            || {},
        );
        slots.offer(
            2,
            async move {
                ran_tx.send(2).expect("the test reads the log");
            },
            || {},
        );
        sleep(Duration::from_millis(50)).await;
        assert!(ran.try_recv().is_err(), "the second item waits for the only slot");
        open.send(()).expect("the first item waits on its gate");
        pretty_assertions::assert_eq!(within(ran.recv()).await, Some(1));
        pretty_assertions::assert_eq!(within(ran.recv()).await, Some(2));
    }

    #[tokio::test]
    async fn finishing_stops_running_and_queued_work_and_frees_every_key_without_callbacks() {
        let slots = slots(1);
        let (freed_tx, mut freed) = mpsc::unbounded_channel::<()>();
        let refused = freed_tx.clone(); // clone: both work items report through one channel
        slots.offer(1, future::pending(), move || refused.send(()).expect("the test reads the callbacks"));
        slots.offer(2, future::pending(), move || freed_tx.send(()).expect("the test reads the callbacks"));
        pretty_assertions::assert_eq!(slots.offer(1, async {}, || {}), Admission::Busy);

        within(slots.finish()).await;

        assert!(!slots.is_in_flight(&1) && !slots.is_in_flight(&2), "cancelled work frees its key");
        assert!(freed.try_recv().is_err(), "cancelled work tells no owner to offer again");
    }
}
