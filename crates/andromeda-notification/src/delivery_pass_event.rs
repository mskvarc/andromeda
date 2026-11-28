//! What a delivery pass waits for: the next event of its in-flight set, or,
//! while a commit could change what it claims, a commit's wake.

use crate::{delivery_pass_state::CommitWatch, delivery_slot_event::SlotEvent};
use futures::{
    StreamExt,
    future::{BoxFuture, Either, select},
    stream::{Fuse, FusedStream, FuturesUnordered, Stream},
};
use std::pin::Pin;

/// What a pass waited for.
pub(crate) enum PassEvent {
    /// Something in the in-flight set finished.
    Finished(SlotEvent),
    /// A commit may have queued deliveries.
    Committed,
    /// Nothing is in flight, so the pass is over.
    Idle,
}

/// Wait for the next event of `slots`, and under [`CommitWatch::Watching`]
/// for the next wake on `commits` too, until `commits` ends.
pub(crate) async fn next_pass_event<S: Stream<Item = ()>>(
    slots: &mut FuturesUnordered<BoxFuture<'_, SlotEvent>>,
    mut commits: Pin<&mut Fuse<S>>,
    watch: CommitWatch,
) -> PassEvent {
    let finished = |event: Option<SlotEvent>| event.map_or(PassEvent::Idle, PassEvent::Finished);
    match watch {
        CommitWatch::Watching if !commits.is_terminated() => match select(slots.next(), commits.next()).await {
            Either::Left((event, _)) => finished(event),
            Either::Right((Some(()), _)) => PassEvent::Committed,
            Either::Right((None, slot)) => finished(slot.await),
        },
        CommitWatch::Watching | CommitWatch::SlotsOnly => finished(slots.next().await),
    }
}
