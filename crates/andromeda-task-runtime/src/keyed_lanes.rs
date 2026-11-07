//! Bounded concurrent execution of work items that keeps the items sharing a
//! key in the order they were dispatched.
//!
//! Each key hashes onto one of a fixed number of lanes, and each lane runs its
//! items one at a time, so two items of one key never overlap or reorder while
//! items of keys on other lanes proceed beside them. A lane holds a bounded
//! queue: dispatching onto a full lane waits for it to drain, which hands the
//! backpressure to whatever feeds the dispatcher.

use std::{
    hash::{BuildHasher, Hash, RandomState},
    num::NonZeroUsize,
    sync::Arc,
};
use thiserror::Error;
use tokio::{sync::mpsc, task::JoinSet};
use tracing::warn;

/// A dispatched item could not be queued: the lane its key hashes onto has
/// stopped running.
///
/// A lane stops only when its worker panicked or was aborted, so the lanes can
/// no longer keep their order promise and are started afresh by their owner.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("the lane of a dispatched item has stopped")]
pub struct LaneStopped;

/// Fixed-width lanes running one worker each.
///
/// Dropping the lanes aborts every worker and discards the items still queued;
/// [`KeyedLanes::finish`] runs them out first.
#[derive(Debug)]
pub struct KeyedLanes<T> {
    /// The queue of each lane, indexed by lane.
    lanes: Box<[mpsc::Sender<T>]>,
    /// Places a key on a lane; seeded per instance, so a client choosing keys
    /// cannot aim them all at one lane.
    placement: RandomState,
    /// The lane workers; dropping the set aborts them.
    workers: JoinSet<()>,
}

impl<T: Send + 'static> KeyedLanes<T> {
    /// Start `width` lanes, each queueing up to `depth` items and running
    /// `work` on them one at a time.
    ///
    /// The workers are spawned on the Tokio runtime the caller runs on.
    pub fn start<W, F>(width: NonZeroUsize, depth: NonZeroUsize, work: W) -> KeyedLanes<T>
    where
        W: Fn(T) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let work = Arc::new(work);
        let mut workers = JoinSet::new();
        let lanes = (0..width.get())
            .map(|_| {
                let (sender, mut receiver) = mpsc::channel::<T>(depth.get());
                let work = Arc::clone(&work); // clone: every lane worker runs the one shared work function.
                workers.spawn(async move {
                    while let Some(item) = receiver.recv().await {
                        work(item).await;
                    }
                });
                sender
            })
            .collect();
        KeyedLanes {
            lanes,
            placement: RandomState::new(),
            workers,
        }
    }

    /// The lane `key` is placed on.
    ///
    /// The key is read only here, so it may be borrowed from the item then
    /// dispatched on the lane.
    pub fn lane<K: Hash + ?Sized>(&self, key: &K) -> Lane<'_, T> {
        Lane { queue: self.lane_of(key) }
    }

    /// Accept no more items and wait until every lane has run what it queued.
    pub async fn finish(self) {
        let KeyedLanes { lanes, mut workers, .. } = self;
        drop(lanes);
        while let Some(joined) = workers.join_next().await {
            if let Err(error) = joined {
                warn!(%error, "a keyed lane worker ended before running out its queue");
            }
        }
    }

    /// The lane `key` is placed on; every key has one, since there is at least
    /// one lane and the slot is the hash modulo their number.
    fn lane_of<K: Hash + ?Sized>(&self, key: &K) -> Option<&mpsc::Sender<T>> {
        let width = u64::try_from(self.lanes.len()).ok()?;
        let slot = usize::try_from(self.placement.hash_one(key) % width).ok()?;
        self.lanes.get(slot)
    }
}

/// One lane of [`KeyedLanes`], chosen by the key of the items queued on it.
#[derive(Debug)]
pub struct Lane<'a, T> {
    /// The lane's queue; `None` only if the key was placed on no lane, which
    /// at least one lane rules out.
    queue: Option<&'a mpsc::Sender<T>>,
}

impl<T> Lane<'_, T> {
    /// Queue `item` on this lane, waiting while the lane is full.
    ///
    /// # Errors
    ///
    /// Returns [`LaneStopped`] when the lane's worker no longer runs; the item
    /// is dropped.
    pub async fn dispatch(self, item: T) -> Result<(), LaneStopped> {
        let queue = self.queue.ok_or(LaneStopped)?;
        queue.send(item).await.map_err(|_| LaneStopped)
    }
}

#[cfg(test)]
mod tests {
    use crate::keyed_lanes::{KeyedLanes, LaneStopped};
    use std::{num::NonZeroUsize, ptr, time::Duration};
    use tokio::{
        sync::{mpsc, oneshot},
        time::timeout,
    };

    /// One test item: a label to log once it ran, and a gate it waits on first.
    struct Item {
        label: &'static str,
        gate: Option<oneshot::Receiver<()>>,
    }

    impl Item {
        const fn open(label: &'static str) -> Item {
            Item { label, gate: None }
        }
    }

    fn logging_lanes(width: usize) -> (KeyedLanes<Item>, mpsc::UnboundedReceiver<&'static str>) {
        let (log, ran) = mpsc::unbounded_channel();
        let lanes = KeyedLanes::start(
            NonZeroUsize::new(width).expect("a positive width"),
            NonZeroUsize::new(4).expect("a positive depth"),
            move |item: Item| {
                let log = log.clone(); // clone: each run logs through the shared sender
                async move {
                    if let Some(gate) = item.gate {
                        gate.await.expect("the test opens every gate it sets");
                    }
                    log.send(item.label).expect("the test reads the log");
                }
            },
        );
        (lanes, ran)
    }

    /// A key placed on another lane than `taken`'s.
    fn key_beside(lanes: &KeyedLanes<Item>, taken: &str) -> String {
        let taken = lanes.lane_of(taken).expect("every key has a lane");
        (0..1_000)
            .map(|n| format!("other-{n}"))
            .find(|key| !ptr::eq(lanes.lane_of(key.as_str()).expect("every key has a lane"), taken))
            .expect("a second lane holds some key")
    }

    async fn next(ran: &mut mpsc::UnboundedReceiver<&'static str>) -> &'static str {
        timeout(Duration::from_secs(5), ran.recv())
            .await
            .expect("an item runs within the timeout")
            .expect("the lanes still log")
    }

    #[tokio::test]
    async fn an_item_waits_for_the_earlier_item_of_its_key_while_another_lane_proceeds() {
        let (lanes, mut ran) = logging_lanes(4);
        let other = key_beside(&lanes, "entity");
        let (open_first, gate) = oneshot::channel();
        lanes
            .lane("entity")
            .dispatch(Item {
                label: "first",
                gate: Some(gate),
            })
            .await
            .expect("the lane runs");
        lanes.lane("entity").dispatch(Item::open("second")).await.expect("the lane runs");
        lanes.lane(other.as_str()).dispatch(Item::open("beside")).await.expect("the lane runs");

        pretty_assertions::assert_eq!(next(&mut ran).await, "beside");
        open_first.send(()).expect("the first item waits on its gate");
        pretty_assertions::assert_eq!(next(&mut ran).await, "first");
        pretty_assertions::assert_eq!(next(&mut ran).await, "second");
    }

    #[tokio::test]
    async fn items_of_one_key_run_in_dispatch_order() {
        const LABELS: [&str; 6] = ["a", "b", "c", "d", "e", "f"];
        let (lanes, mut ran) = logging_lanes(3);
        for label in LABELS {
            lanes.lane("entity").dispatch(Item::open(label)).await.expect("the lane runs");
        }
        let mut order = Vec::new();
        for _ in LABELS {
            order.push(next(&mut ran).await);
        }
        pretty_assertions::assert_eq!(order, LABELS);
    }

    #[tokio::test]
    async fn finishing_runs_out_every_queued_item() {
        let (lanes, mut ran) = logging_lanes(2);
        lanes.lane("entity").dispatch(Item::open("queued")).await.expect("the lane runs");
        lanes.lane("other").dispatch(Item::open("beside")).await.expect("the lane runs");
        timeout(Duration::from_secs(5), lanes.finish())
            .await
            .expect("the lanes finish within the timeout");
        let mut finished = vec![next(&mut ran).await, next(&mut ran).await];
        finished.sort_unstable();
        pretty_assertions::assert_eq!(finished, ["beside", "queued"]);
        assert!(ran.recv().await.is_none(), "the finished lanes dropped their work, and with it the log");
    }

    #[tokio::test]
    async fn an_item_for_a_stopped_lane_is_refused() {
        let (mut lanes, _ran) = logging_lanes(2);
        lanes.workers.abort_all();
        while lanes.workers.join_next().await.is_some() {}
        pretty_assertions::assert_eq!(lanes.lane("entity").dispatch(Item::open("late")).await, Err(LaneStopped));
    }
}
