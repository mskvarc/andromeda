//! The hand-off of committed changes to the bounded queue a tenant's
//! notification evaluation reads.
//!
//! A notification is owed for every committed change (clause 5.8.6 for an
//! Attribute of an Entity, clause 5.11.7 for a context source registration), so
//! the queue never drops one: a write that finds it full waits for room. A
//! change queues behind every change already waiting for room, since a full
//! queue hands each freed slot to the longest waiter, so the changes of one
//! write stay in commit order. Once a write waits, a task of its own finishes
//! the hand-off: the changes have committed, and abandoning the request must
//! not lose them. Those tasks belong to a tracker the tenant owns, so stopping
//! the tenant waits for every one of them, and [`settle`] reads the queue empty
//! before its reader stops.

use futures::{future, future::BoxFuture};
use std::{future::Future, iter};
use strum::Display;
use tokio::sync::{mpsc, mpsc::error::TrySendError};
use tokio_util::task::TaskTracker;
use tracing::{debug, warn};

/// The kind of change a queue carries, as its log lines name it.
#[derive(Clone, Copy, Debug, Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum QueuedChange {
    /// Committed changes of the tenant's entities.
    Entity,
    /// Committed changes of the tenant's context source registrations.
    Registration,
}

/// Queue every one of `changes` on `sender` in order, resolving once the last
/// of them is queued.
///
/// The changes that fit are queued before this returns; the rest are queued
/// from a task of `handoffs`, which the returned future waits for.
pub(crate) fn queue_in_order<T, I>(sender: &mpsc::Sender<T>, handoffs: &TaskTracker, changes: I, kind: QueuedChange) -> BoxFuture<'static, ()>
where
    T: Send + 'static,
    I: IntoIterator<Item = T>,
    I::IntoIter: Send + 'static,
{
    let mut changes = changes.into_iter();
    match changes.by_ref().find_map(|change| sender.try_send(change).err()) {
        None => Box::pin(future::ready(())),
        Some(TrySendError::Full(change)) => {
            wait_for_room(sender.clone(), handoffs, iter::once(change).chain(changes), kind) // clone: the hand-off task owns its sender handle.
        }
        Some(TrySendError::Closed(_)) => {
            debug!(changes = %kind, "no notification evaluation receives the committed changes");
            Box::pin(future::ready(()))
        }
    }
}

/// Queue `changes` from a task of `handoffs`, resolving once the last of them
/// is queued.
fn wait_for_room<T, I>(sender: mpsc::Sender<T>, handoffs: &TaskTracker, changes: I, kind: QueuedChange) -> BoxFuture<'static, ()>
where
    T: Send + 'static,
    I: Iterator<Item = T> + Send + 'static,
{
    let handoff = handoffs.spawn(async move {
        for change in changes {
            if sender.send(change).await.is_err() {
                debug!(changes = %kind, "no notification evaluation receives the committed changes");
                return;
            }
        }
    });
    Box::pin(async move {
        if let Err(error) = handoff.await {
            warn!(changes = %kind, %error, "the hand-off of committed changes to notification evaluation did not finish");
        }
    })
}

/// Hand every committed change a stopping tenant still owes to `accept`: the
/// changes waiting in `receiver` and those its hand-offs are still queueing.
///
/// The queue is read while the hand-offs finish, since a hand-off waits for the
/// room only the reader frees. Once none runs, the queue is closed and what it
/// holds is read out, so no change is lost to the reader stopping.
///
/// # Errors
///
/// Returns the first error `accept` fails with; the changes behind it stay
/// unread.
pub(crate) async fn settle<T, E, F, Fut>(handoffs: &TaskTracker, receiver: &mut mpsc::Receiver<T>, mut accept: F) -> Result<(), E>
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = Result<(), E>>,
{
    handoffs.close();
    let finished = handoffs.wait();
    tokio::pin!(finished);
    loop {
        tokio::select! {
            () = &mut finished => break,
            queued = receiver.recv() => match queued {
                Some(change) => accept(change).await?,
                None => break,
            },
        }
    }
    receiver.close();
    while let Ok(change) = receiver.try_recv() {
        accept(change).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::event::queued_handoff::{QueuedChange, queue_in_order, settle};
    use futures::future;
    use std::{
        convert::Infallible,
        io::{self, Write},
        sync::{Arc, Mutex},
    };
    use tokio::sync::mpsc;
    use tokio_util::task::TaskTracker;
    use tracing::{
        Level,
        subscriber::{DefaultGuard, set_default},
    };
    use tracing_subscriber::fmt::MakeWriter;

    async fn settled(handoffs: &TaskTracker, receiver: &mut mpsc::Receiver<u32>) -> Vec<u32> {
        let mut arrived = Vec::new();
        settle(handoffs, receiver, |change| {
            arrived.push(change);
            future::ready(Ok::<(), Infallible>(()))
        })
        .await
        .expect("accepting a change never fails");
        arrived
    }

    #[tokio::test]
    async fn settling_reads_out_the_changes_still_in_the_queue() {
        let (sender, mut receiver) = mpsc::channel(4);
        let handoffs = TaskTracker::new();
        queue_in_order(&sender, &handoffs, [1, 2, 3], QueuedChange::Entity).await;
        assert_eq!(settled(&handoffs, &mut receiver).await, [1, 2, 3]);
    }

    #[tokio::test]
    async fn settling_reads_while_a_handoff_waits_for_room_and_loses_no_change() {
        let (sender, mut receiver) = mpsc::channel(1);
        let handoffs = TaskTracker::new();
        drop(queue_in_order(&sender, &handoffs, [1, 2, 3, 4, 5], QueuedChange::Entity));
        assert_eq!(settled(&handoffs, &mut receiver).await, [1, 2, 3, 4, 5]);
        assert!(handoffs.is_empty(), "no hand-off is still running once the queue is settled");
    }

    #[tokio::test]
    async fn settling_closes_the_queue_to_later_changes() {
        let (sender, mut receiver) = mpsc::channel(4);
        let handoffs = TaskTracker::new();
        settled(&handoffs, &mut receiver).await;
        assert!(sender.try_send(1).is_err(), "a settled queue takes no further change");
    }

    #[tokio::test]
    async fn settling_stops_at_the_first_refused_change() {
        let (sender, mut receiver) = mpsc::channel(4);
        let handoffs = TaskTracker::new();
        queue_in_order(&sender, &handoffs, [1, 2, 3], QueuedChange::Entity).await;
        let result = settle(&handoffs, &mut receiver, |change| future::ready(if change == 2 { Err(change) } else { Ok(()) })).await;
        assert_eq!(result, Err(2));
    }

    /// A log sink the test reads back.
    #[derive(Clone, Default)]
    struct CapturedLog(Arc<Mutex<Vec<u8>>>);

    impl Write for CapturedLog {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| io::Error::other("the log buffer is poisoned"))?
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for CapturedLog {
        type Writer = CapturedLog;

        fn make_writer(&'writer self) -> CapturedLog {
            self.clone()
        }
    }

    impl CapturedLog {
        fn text(&self) -> String {
            self.0.lock().map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default()
        }
    }

    /// Capture what the test logs, for as long as the returned guard lives.
    fn captured_log() -> (CapturedLog, DefaultGuard) {
        let log = CapturedLog::default();
        let guard = set_default(tracing_subscriber::fmt().with_writer(log.clone()).with_max_level(Level::DEBUG).finish());
        (log, guard)
    }

    const NO_RECEIVER_LINE: &str = "no notification evaluation receives the committed changes";

    #[tokio::test]
    async fn a_graceful_stop_logs_no_dropped_change() {
        let (log, _guard) = captured_log();
        let (sender, mut receiver) = mpsc::channel(1);
        let handoffs = TaskTracker::new();
        drop(queue_in_order(&sender, &handoffs, [1, 2, 3, 4], QueuedChange::Entity));
        assert_eq!(settled(&handoffs, &mut receiver).await, [1, 2, 3, 4]);
        assert!(
            !log.text().contains(NO_RECEIVER_LINE),
            "a stop that reads every change out drops none: {}",
            log.text()
        );
    }

    #[tokio::test]
    async fn a_change_handed_over_after_the_queue_closed_is_logged_as_dropped() {
        let (log, _guard) = captured_log();
        let (sender, receiver) = mpsc::channel::<u32>(1);
        drop(receiver);
        queue_in_order(&sender, &TaskTracker::new(), [1], QueuedChange::Entity).await;
        assert!(log.text().contains(NO_RECEIVER_LINE));
    }
}
