//! The hand-off of committed registration changes to a tenant's context
//! source notification evaluation.
//!
//! Clause 5.11.7 owes a notification for every change of a matching
//! registration, so a change is queued as [`queue_in_order`] queues it: never
//! dropped, in commit order.

use crate::event::queued_handoff::{QueuedChange, queue_in_order};
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_registration::RegistrationChangeSink;
use futures::future::BoxFuture;
use std::iter;
use tokio::sync::mpsc;
use tokio_util::task::TaskTracker;

/// Queues committed registration changes for the tenant's context source
/// notification evaluation.
#[derive(Clone, Debug)]
pub(crate) struct QueuedRegistrationChangeSink {
    /// The queue context source notification evaluation reads.
    sender: mpsc::Sender<CsrChangeEvent>,
    /// The tasks finishing the hand-off of writes that found the queue full.
    handoffs: TaskTracker,
}

impl QueuedRegistrationChangeSink {
    /// Create a sink queueing into the supplied sender.
    pub(crate) fn new(sender: mpsc::Sender<CsrChangeEvent>) -> QueuedRegistrationChangeSink {
        QueuedRegistrationChangeSink {
            sender,
            handoffs: TaskTracker::new(),
        }
    }

    /// The tasks finishing the hand-off of writes that found the queue full;
    /// stopping the tenant waits for them.
    pub(crate) const fn handoffs(&self) -> &TaskTracker {
        &self.handoffs
    }
}

impl RegistrationChangeSink for QueuedRegistrationChangeSink {
    fn publish(&self, event: CsrChangeEvent) -> BoxFuture<'static, ()> {
        queue_in_order(&self.sender, &self.handoffs, iter::once(event), QueuedChange::Registration)
    }
}

#[cfg(test)]
mod tests {
    use crate::event::registration_change_sink::QueuedRegistrationChangeSink;
    use andromeda_ngsi_ld_model::{
        csource_change_revision::CsourceChangeRevision,
        notification::{CsrChange, CsrChangeEvent},
    };
    use andromeda_registration::RegistrationChangeSink;
    use futures::{FutureExt, future};
    use tokio::sync::mpsc;

    fn deletion(registration: i64) -> CsrChangeEvent {
        CsrChangeEvent {
            csr_id: format!("urn:ngsi-ld:ContextSourceRegistration:{registration}")
                .parse()
                .expect("a valid identifier"),
            change: CsrChange::Deleted { before: None },
            revision: CsourceChangeRevision::from(registration),
        }
    }

    async fn drain(queue: &mut mpsc::Receiver<CsrChangeEvent>, count: usize) -> Vec<String> {
        let mut identifiers = Vec::with_capacity(count);
        while identifiers.len() < count {
            let event = queue.recv().await.expect("the sink keeps its sender");
            identifiers.push(event.csr_id.to_string());
        }
        identifiers
    }

    #[tokio::test]
    async fn a_change_that_fits_is_queued_before_publish_returns() {
        let (sender, mut receiver) = mpsc::channel(4);
        let sink = QueuedRegistrationChangeSink::new(sender);
        sink.publish(deletion(1)).now_or_never().expect("a queue with room takes the change at once");
        assert_eq!(drain(&mut receiver, 1).await, ["urn:ngsi-ld:ContextSourceRegistration:1"]);
    }

    /// Clause 5.11.7 owes a notification for every change of a matching
    /// registration, so a burst larger than the queue waits for evaluation to
    /// catch up instead of dropping the oldest changes.
    #[tokio::test]
    async fn a_burst_past_the_capacity_waits_and_loses_no_change() {
        let (sender, mut receiver) = mpsc::channel(4);
        let sink = QueuedRegistrationChangeSink::new(sender);
        let mut publishes: Vec<_> = (0..11).map(|registration| sink.publish(deletion(registration))).collect();
        let last = publishes.last_mut().expect("the burst holds changes");
        assert!(last.now_or_never().is_none(), "a burst larger than the queue waits for evaluation to catch up");
        let arrived = drain(&mut receiver, 11).await;
        future::join_all(publishes).await;
        let expected: Vec<_> = (0..11)
            .map(|registration| format!("urn:ngsi-ld:ContextSourceRegistration:{registration}"))
            .collect();
        assert_eq!(arrived, expected, "every change arrives, in commit order");
    }

    #[tokio::test]
    async fn an_abandoned_wait_still_queues_the_change() {
        let (sender, mut receiver) = mpsc::channel(1);
        let sink = QueuedRegistrationChangeSink::new(sender);
        sink.publish(deletion(1)).await;
        drop(sink.publish(deletion(2)));
        assert_eq!(
            drain(&mut receiver, 2).await,
            ["urn:ngsi-ld:ContextSourceRegistration:1", "urn:ngsi-ld:ContextSourceRegistration:2"],
            "a committed change reaches evaluation although its request was abandoned"
        );
    }
}
