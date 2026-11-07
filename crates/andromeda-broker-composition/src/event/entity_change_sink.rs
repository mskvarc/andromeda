//! The hand-off of committed entity changes to a tenant's live notification
//! evaluation.
//!
//! Clause 5.8.6 owes a notification for every change of a watched Attribute,
//! and clause 5.5.11.3 a separate one for each update, so a change is queued
//! as [`queue_in_order`] queues it: never dropped, in commit order. Every change
//! is held among the tenant's pending commits from the moment it is handed
//! off, including while it waits for room, until it is evaluated or dropped.

use crate::event::queued_handoff::{QueuedChange, queue_in_order};
use andromeda_context_commit::context_change_event::ContextChangeEvent;
use andromeda_entity::change_sink::EntityChangeSink;
use andromeda_notification::pending_change::{PendingChange, PendingCommits};
use futures::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::task::TaskTracker;

/// Queues committed entity changes for the tenant's live notification
/// evaluation.
#[derive(Clone, Debug)]
pub(crate) struct QueuedEntityChangeSink {
    /// The queue live evaluation reads.
    sender: mpsc::Sender<PendingChange>,
    /// The changes handed off and not yet evaluated.
    commits: Arc<PendingCommits>,
    /// The tasks finishing the hand-off of writes that found the queue full.
    handoffs: TaskTracker,
}

impl QueuedEntityChangeSink {
    /// Create a sink queueing into `sender`, holding every change among
    /// `commits` until it is dropped.
    pub(crate) fn new(sender: mpsc::Sender<PendingChange>, commits: Arc<PendingCommits>) -> QueuedEntityChangeSink {
        QueuedEntityChangeSink {
            sender,
            commits,
            handoffs: TaskTracker::new(),
        }
    }

    /// The tasks finishing the hand-off of writes that found the queue full;
    /// stopping the tenant waits for them.
    pub(crate) const fn handoffs(&self) -> &TaskTracker {
        &self.handoffs
    }

    /// The changes handed off through this sink and not yet evaluated.
    pub(crate) const fn commits(&self) -> &Arc<PendingCommits> {
        &self.commits
    }
}

impl EntityChangeSink for QueuedEntityChangeSink {
    fn publish(&self, changes: Vec<ContextChangeEvent>) -> BoxFuture<'static, ()> {
        let held: Vec<PendingChange> = changes.into_iter().map(|change| self.commits.hold(change)).collect();
        queue_in_order(&self.sender, &self.handoffs, held, QueuedChange::Entity)
    }
}

#[cfg(test)]
mod tests {
    use crate::event::{entity_change_sink::QueuedEntityChangeSink, queued_handoff::settle};
    use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent, store_instant::StoreInstant};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::change_sink::EntityChangeSink;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
    };
    use andromeda_notification::pending_change::{PendingChange, PendingCommits};
    use chrono::Utc;
    use futures::{FutureExt, future};
    use std::{convert::Infallible, sync::Arc};
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
            committed_at: StoreInstant::new(Utc::now()),
        }
    }

    async fn drain(queue: &mut mpsc::Receiver<PendingChange>, count: usize) -> Vec<String> {
        let mut identifiers = Vec::with_capacity(count);
        while identifiers.len() < count {
            let pending = queue.recv().await.expect("the sink keeps its sender");
            identifiers.push(pending.change.event.entity_id.to_string());
        }
        identifiers
    }

    fn sink(sender: mpsc::Sender<PendingChange>) -> QueuedEntityChangeSink {
        QueuedEntityChangeSink::new(sender, Arc::new(PendingCommits::default()))
    }

    /// A change waiting for room is not yet evaluated, so it bounds which
    /// superseded selections are kept as much as one already queued.
    #[tokio::test]
    async fn a_change_is_held_from_its_hand_off_until_it_is_dropped() {
        let (sender, mut receiver) = mpsc::channel(1);
        let sink = sink(sender);
        let first = change("1");
        let committed_at = first.committed_at;
        let waiting = sink.publish(vec![first, change("2")]);
        assert_eq!(sink.commits().floor(), Some(committed_at));
        let queued = receiver.recv().await.expect("the first change is queued");
        let second = receiver.recv().await.expect("the second change is queued");
        waiting.await;
        drop(queued);
        assert!(sink.commits().floor().is_some(), "the second change is still held");
        drop(second);
        assert_eq!(sink.commits().floor(), None);
    }

    #[tokio::test]
    async fn changes_that_fit_are_queued_before_publish_returns() {
        let (sender, mut receiver) = mpsc::channel(4);
        let sink = sink(sender);
        sink.publish(vec![change("1"), change("2")])
            .now_or_never()
            .expect("a queue with room takes the changes at once");
        assert_eq!(drain(&mut receiver, 2).await, ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]);
    }

    #[tokio::test]
    async fn a_burst_past_the_capacity_waits_and_loses_no_change() {
        let (sender, mut receiver) = mpsc::channel(4);
        let sink = sink(sender);
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
        let sink = sink(sender);
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
        let sink = sink(sender);
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

    #[tokio::test]
    async fn a_saturated_queue_holds_one_handoff_per_waiting_write() {
        let (sender, mut receiver) = mpsc::channel(1);
        let sink = sink(sender);
        sink.publish(vec![change("0")]).await;
        let waiting: Vec<_> = (1..=5).map(|write| sink.publish(vec![change(&write.to_string())])).collect();
        assert_eq!(sink.handoffs().len(), 5, "each waiting write holds exactly one hand-off");
        let mut arrived = Vec::new();
        settle(sink.handoffs(), &mut receiver, |pending| {
            arrived.push(pending.change.event.entity_id.to_string());
            future::ready(Ok::<(), Infallible>(()))
        })
        .await
        .expect("accepting a change never fails");
        future::join_all(waiting).await;
        assert_eq!(arrived.len(), 6, "every committed change is read out when the queue settles");
        assert!(sink.handoffs().is_empty(), "no hand-off is still running after the queue settles");
    }

    #[tokio::test]
    async fn settling_keeps_the_commit_order_of_changes_still_waiting_for_room() {
        let (sender, mut receiver) = mpsc::channel(1);
        let sink = sink(sender);
        drop(sink.publish(vec![change("1"), change("2"), change("3")]));
        let mut arrived = Vec::new();
        settle(sink.handoffs(), &mut receiver, |pending| {
            arrived.push(pending.change.event.entity_id.to_string());
            future::ready(Ok::<(), Infallible>(()))
        })
        .await
        .expect("accepting a change never fails");
        assert_eq!(arrived, ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"]);
    }
}
