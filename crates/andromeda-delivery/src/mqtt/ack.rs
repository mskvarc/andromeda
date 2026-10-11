//! Correlating an outgoing MQTT publish with the broker acknowledgement that
//! completes it.
//!
//! `rumqttc`'s `publish` returns before the packet is even assigned an
//! identifier, so a caller cannot key on the packet id to await its
//! acknowledgement. The event loop is the only place that sees both the
//! `Outgoing::Publish(pkid)` when a publish reaches the wire and the `PubAck`
//! (`QoS` 1) or `PubComp` (`QoS` 2) that later completes it. Publishes are
//! registered here in send order (the delivery path holds a lock across
//! register-and-publish, so the registration order matches the wire order),
//! assigned their id when the event loop reports the outgoing packet, and
//! resolved when the matching acknowledgement arrives.

use std::collections::{HashMap, VecDeque};
use tokio::sync::oneshot;

/// The acknowledgement packet that completes a publish at its `QoS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckKind {
    /// `QoS` 1 completes on `PUBACK`.
    PubAck,
    /// `QoS` 2 completes on `PUBCOMP`, after the `PUBREC`/`PUBREL` handshake.
    PubComp,
}

/// One publish awaiting its broker acknowledgement.
#[derive(Debug)]
struct PendingPublish {
    /// The packet whose arrival completes this publish.
    completes_on: AckKind,
    /// Resolved once that packet arrives; dropped if the connection fails first,
    /// which the delivery path reads as a retryable failure.
    sender: oneshot::Sender<()>,
}

/// Correlates outgoing publishes with their acknowledgements for one connection.
#[derive(Debug, Default)]
pub struct AckTracker {
    /// Publishes registered but not yet seen on the wire, in send order.
    unassigned: VecDeque<PendingPublish>,
    /// Publishes on the wire, keyed by the packet id the event loop assigned.
    in_flight: HashMap<u16, PendingPublish>,
}

impl AckTracker {
    /// Register a publish about to be sent, returning the receiver that resolves
    /// when the broker acknowledges it.
    pub fn register(&mut self, completes_on: AckKind) -> oneshot::Receiver<()> {
        let (sender, receiver) = oneshot::channel();
        self.unassigned.push_back(PendingPublish { completes_on, sender });
        receiver
    }

    /// Drop the most recently registered publish, for a `publish` call that
    /// failed before its packet reached the wire.
    ///
    /// Sound only under the delivery path's publish lock, which is what makes
    /// "most recent" the publish that just failed rather than a concurrent one.
    pub fn cancel_last_registered(&mut self) {
        self.unassigned.pop_back();
    }

    /// Assign the next unsent publish the packet id the event loop reported for
    /// its `Outgoing::Publish`.
    ///
    /// A packet identifier is non-zero (MQTT 3.1.1 section 2.3.1, MQTT 5.0
    /// section 2.2.1). A `QoS` 0 publish carries none, the client reports it as
    /// 0, and it is never registered, so it takes no waiter.
    pub fn on_outgoing_publish(&mut self, pkid: u16) {
        if pkid == 0 {
            return;
        }
        if let Some(pending) = self.unassigned.pop_front() {
            self.in_flight.insert(pkid, pending);
        }
    }

    /// Complete the publish a `PUBACK` acknowledges (`QoS` 1).
    pub fn on_puback(&mut self, pkid: u16) {
        self.complete(pkid, AckKind::PubAck);
    }

    /// Complete the publish a `PUBCOMP` acknowledges (`QoS` 2).
    pub fn on_pubcomp(&mut self, pkid: u16) {
        self.complete(pkid, AckKind::PubComp);
    }

    /// Resolve the in-flight publish for `pkid`, but only when `kind` is the
    /// acknowledgement it waits on: a `PUBACK` for a `QoS` 2 publish still
    /// mid-handshake leaves it in flight for its real `PUBCOMP`.
    fn complete(&mut self, pkid: u16, kind: AckKind) {
        let awaits_this = self.in_flight.get(&pkid).is_some_and(|pending| pending.completes_on == kind);
        if awaits_this && let Some(pending) = self.in_flight.remove(&pkid) {
            // A dropped receiver means the delivery path already timed out; the
            // acknowledgement is simply late, and there is nothing to resolve.
            let _ = pending.sender.send(());
        }
    }

    /// Fail every waiting publish, for a connection that has gone down.
    ///
    /// Dropping the senders resolves each receiver to a cancellation, which the
    /// delivery path turns into a retryable failure so the delivery queue retransmits.
    pub fn fail_all(&mut self) {
        self.unassigned.clear();
        self.in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    use crate::mqtt::ack::{AckKind, AckTracker};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_qos1_publish_resolves_on_its_puback() {
        let mut tracker = AckTracker::default();
        let mut receiver = tracker.register(AckKind::PubAck);
        tracker.on_outgoing_publish(7);
        tracker.on_puback(7);
        assert_eq!(receiver.try_recv(), Ok(()), "the publish is acknowledged");
    }

    #[test]
    fn a_qos2_publish_resolves_on_pubcomp_not_puback() {
        let mut tracker = AckTracker::default();
        let mut receiver = tracker.register(AckKind::PubComp);
        tracker.on_outgoing_publish(3);
        // A stray PUBACK for the same id must not complete a QoS 2 publish.
        tracker.on_puback(3);
        assert!(receiver.try_recv().is_err(), "PUBACK does not complete a QoS 2 publish");
        tracker.on_pubcomp(3);
        assert_eq!(receiver.try_recv(), Ok(()), "PUBCOMP completes it");
    }

    #[test]
    fn publishes_are_assigned_ids_in_send_order() {
        let mut tracker = AckTracker::default();
        let mut first = tracker.register(AckKind::PubAck);
        let mut second = tracker.register(AckKind::PubAck);
        // The event loop reports the outgoing packets in the order they were sent.
        tracker.on_outgoing_publish(10);
        tracker.on_outgoing_publish(11);
        tracker.on_puback(11);
        tracker.on_puback(10);
        assert_eq!(first.try_recv(), Ok(()));
        assert_eq!(second.try_recv(), Ok(()));
    }

    #[test]
    fn a_qos0_publish_beside_a_qos1_publish_takes_no_waiter() {
        let mut tracker = AckTracker::default();
        let mut receiver = tracker.register(AckKind::PubAck);
        tracker.on_outgoing_publish(0);
        tracker.on_outgoing_publish(0);
        tracker.on_outgoing_publish(4);
        tracker.on_puback(4);
        assert_eq!(receiver.try_recv(), Ok(()), "the QoS 1 publish is acknowledged by its own PUBACK");
    }

    #[test]
    fn a_failed_connection_cancels_every_waiter() {
        let mut tracker = AckTracker::default();
        let mut still_unsent = tracker.register(AckKind::PubAck);
        let mut on_the_wire = tracker.register(AckKind::PubComp);
        // The first registered publish reaches the wire; the second has not yet.
        tracker.on_outgoing_publish(1);
        tracker.fail_all();
        assert!(still_unsent.try_recv().is_err(), "a waiter cancels whether or not it reached the wire");
        assert!(on_the_wire.try_recv().is_err(), "the in-flight waiter cancels too");
    }

    #[test]
    fn a_publish_that_never_reached_the_wire_is_cancelled() {
        let mut tracker = AckTracker::default();
        let mut receiver = tracker.register(AckKind::PubAck);
        tracker.cancel_last_registered();
        assert!(receiver.try_recv().is_err(), "the cancelled waiter never resolves");
    }
}
