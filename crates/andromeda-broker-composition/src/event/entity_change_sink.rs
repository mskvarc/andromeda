//! The hand-off of committed entity changes to a tenant's live notification
//! evaluation.
//!
//! Clause 5.8.6 owes a notification for every change of a watched Attribute,
//! and clause 5.5.11.3 a separate one for each update, so the queue never drops
//! a change: a write that finds it full waits for room. A change queues behind
//! every change already waiting for room, since a full queue hands each freed
//! slot to the longest waiter, so the changes of one write stay in commit
//! order. Once a write waits, a task of its own finishes the hand-off: the
//! changes have committed, and abandoning the request must not lose them.

use andromeda_context_commit::context_change_event::ContextChangeEvent;
use andromeda_entity::change_sink::EntityChangeSink;
use futures::{future, future::BoxFuture};
use std::{iter, vec};
use tokio::sync::{mpsc, mpsc::error::TrySendError};
use tracing::{debug, warn};

/// Queues committed entity changes for the tenant's live notification
/// evaluation.
pub struct QueuedEntityChangeSink {
    sender: mpsc::Sender<ContextChangeEvent>,
}

impl QueuedEntityChangeSink {
    /// Create a sink queueing into the supplied sender.
    pub const fn new(sender: mpsc::Sender<ContextChangeEvent>) -> QueuedEntityChangeSink {
        QueuedEntityChangeSink { sender }
    }
}

impl EntityChangeSink for QueuedEntityChangeSink {
    fn publish(&self, changes: Vec<ContextChangeEvent>) -> BoxFuture<'static, ()> {
        let mut changes = changes.into_iter();
        match changes.by_ref().find_map(|change| self.sender.try_send(change).err()) {
            None => Box::pin(future::ready(())),
            Some(TrySendError::Full(change)) => wait_for_room(self.sender.clone(), change, changes), // clone: the hand-off task owns its sender handle.
            Some(TrySendError::Closed(_)) => {
                debug!("no live notification evaluation receives entity changes");
                Box::pin(future::ready(()))
            }
        }
    }
}

/// Queue `first` and then `rest` from a task of their own, resolving once the
/// last of them is queued.
fn wait_for_room(sender: mpsc::Sender<ContextChangeEvent>, first: ContextChangeEvent, rest: vec::IntoIter<ContextChangeEvent>) -> BoxFuture<'static, ()> {
    let handoff = tokio::spawn(async move {
        for change in iter::once(first).chain(rest) {
            if sender.send(change).await.is_err() {
                debug!("no live notification evaluation receives entity changes");
                return;
            }
        }
    });
    Box::pin(async move {
        if let Err(error) = handoff.await {
            warn!(%error, "the hand-off of committed entity changes to live notification evaluation did not finish");
        }
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code expects on purpose")]
mod tests {
    use crate::event::entity_change_sink::QueuedEntityChangeSink;
    use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::change_sink::EntityChangeSink;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
    };
    use chrono::Utc;
    use futures::FutureExt;
    use tokio::{sync::mpsc, task};

    fn change(entity: &str) -> ContextChangeEvent {
        ContextChangeEvent {
            target: ContextTarget::Live,
            event: ChangeEvent::builder()
                .entity_id(format!("urn:ngsi-ld:Vehicle:{entity}").parse::<Identifier>().expect("a valid identifier"))
                .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .build(),
        }
    }

    async fn drain(queue: &mut mpsc::Receiver<ContextChangeEvent>, count: usize) -> Vec<String> {
        let mut identifiers = Vec::with_capacity(count);
        while identifiers.len() < count {
            let change = queue.recv().await.expect("the sink keeps its sender");
            identifiers.push(change.event.entity_id.to_string());
        }
        identifiers
    }

    #[tokio::test]
    async fn changes_that_fit_are_queued_before_publish_returns() {
        let (sender, mut receiver) = mpsc::channel(4);
        let sink = QueuedEntityChangeSink::new(sender);
        sink.publish(vec![change("1"), change("2")])
            .now_or_never()
            .expect("a queue with room takes the changes at once");
        assert_eq!(drain(&mut receiver, 2).await, ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]);
    }

    #[tokio::test]
    async fn a_burst_past_the_capacity_waits_and_loses_no_change() {
        let (sender, mut receiver) = mpsc::channel(4);
        let sink = QueuedEntityChangeSink::new(sender);
        let burst: Vec<_> = (0..11).map(|entity| change(&entity.to_string())).collect();
        let mut publish = sink.publish(burst);
        assert!(
            (&mut publish).now_or_never().is_none(),
            "a burst larger than the queue waits for evaluation to catch up"
        );
        let arrived = drain(&mut receiver, 11).await;
        publish.await;
        let expected: Vec<_> = (0..11).map(|entity| format!("urn:ngsi-ld:Vehicle:{entity}")).collect();
        assert_eq!(arrived, expected, "every change arrives, in commit order");
    }

    #[tokio::test]
    async fn an_abandoned_wait_still_queues_every_change() {
        let (sender, mut receiver) = mpsc::channel(1);
        let sink = QueuedEntityChangeSink::new(sender);
        drop(sink.publish(vec![change("1"), change("2"), change("3")]));
        assert_eq!(
            drain(&mut receiver, 3).await,
            ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"],
            "a committed change reaches evaluation although its request was abandoned"
        );
    }

    #[tokio::test]
    async fn a_later_write_queues_behind_one_already_waiting() {
        let (sender, mut receiver) = mpsc::channel(1);
        let sink = QueuedEntityChangeSink::new(sender);
        sink.publish(vec![change("1")]).await;
        let waiting = sink.publish(vec![change("2")]);
        task::yield_now().await;
        let later = sink.publish(vec![change("3")]);
        assert_eq!(
            drain(&mut receiver, 3).await,
            ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"]
        );
        waiting.await;
        later.await;
    }
}
