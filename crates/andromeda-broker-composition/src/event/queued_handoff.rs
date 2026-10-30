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
//! not lose them.

use futures::{future, future::BoxFuture};
use std::iter;
use strum::Display;
use tokio::sync::{mpsc, mpsc::error::TrySendError};
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
/// from a task of their own, which the returned future waits for.
pub(crate) fn queue_in_order<T, I>(sender: &mpsc::Sender<T>, changes: I, kind: QueuedChange) -> BoxFuture<'static, ()>
where
    T: Send + 'static,
    I: IntoIterator<Item = T>,
    I::IntoIter: Send + 'static,
{
    let mut changes = changes.into_iter();
    match changes.by_ref().find_map(|change| sender.try_send(change).err()) {
        None => Box::pin(future::ready(())),
        Some(TrySendError::Full(change)) => wait_for_room(sender.clone(), iter::once(change).chain(changes), kind), // clone: the hand-off task owns its sender handle.
        Some(TrySendError::Closed(_)) => {
            debug!(changes = %kind, "no notification evaluation receives the committed changes");
            Box::pin(future::ready(()))
        }
    }
}

/// Queue `changes` from a task of their own, resolving once the last of them
/// is queued.
fn wait_for_room<T, I>(sender: mpsc::Sender<T>, changes: I, kind: QueuedChange) -> BoxFuture<'static, ()>
where
    T: Send + 'static,
    I: Iterator<Item = T> + Send + 'static,
{
    let handoff = tokio::spawn(async move {
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
