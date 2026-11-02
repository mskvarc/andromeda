//! When a delivery pass acknowledges, and which settlements go together.
//!
//! A pass has at most one acknowledgement in flight. A delivery whose attempt
//! ends while one is in flight waits for it, and the next acknowledgement
//! carries every settlement that waited. With endpoints that answer at once,
//! the attempts that end during one acknowledgement round trip share the next
//! one instead of each paying a transaction of its own, and a lone settlement
//! with no acknowledgement in flight is still recorded at once, with no timer.

use crate::delivery_round_trip::RoundTrip;
use std::mem::take;

/// The settlements a pass has not acknowledged yet, and its acknowledgement
/// in flight.
#[derive(Debug)]
pub(crate) struct AcknowledgementGroups<T> {
    /// Settlements waiting for the next acknowledgement, in the order their
    /// attempts ended.
    waiting: Vec<T>,
    /// Whether an acknowledgement is in flight.
    acknowledgement: RoundTrip,
}

impl<T> AcknowledgementGroups<T> {
    /// No settlement waiting and no acknowledgement in flight.
    pub(crate) const fn new() -> AcknowledgementGroups<T> {
        AcknowledgementGroups {
            waiting: Vec::new(),
            acknowledgement: RoundTrip::Settled,
        }
    }

    /// Record a delivery whose attempt ended, to be acknowledged.
    pub(crate) fn wait(&mut self, settlement: T) {
        self.waiting.push(settlement);
    }

    /// Record the acknowledgement in flight coming back, whatever it
    /// answered.
    pub(crate) const fn settle(&mut self) {
        self.acknowledgement = RoundTrip::Settled;
    }

    /// The group to acknowledge now, every settlement waiting, or `None`
    /// while an acknowledgement is in flight or none is waiting. A group
    /// returned counts as an acknowledgement in flight until
    /// [`AcknowledgementGroups::settle`].
    pub(crate) fn next_group(&mut self) -> Option<Vec<T>> {
        match (self.acknowledgement, self.waiting.is_empty()) {
            (RoundTrip::InFlight, _) | (RoundTrip::Settled, true) => None,
            (RoundTrip::Settled, false) => {
                self.acknowledgement = RoundTrip::InFlight;
                Some(take(&mut self.waiting))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_acknowledgement_group::AcknowledgementGroups;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_lone_settlement_with_no_acknowledgement_in_flight_is_acknowledged_at_once() {
        let mut groups = AcknowledgementGroups::new();
        groups.wait(1);
        assert_eq!(groups.next_group(), Some(vec![1]));
    }

    #[test]
    fn five_settlements_during_an_acknowledgement_produce_one_further_acknowledgement_of_five() {
        let mut groups = AcknowledgementGroups::new();
        groups.wait(1);
        assert_eq!(groups.next_group(), Some(vec![1]));
        for settlement in 2..=6 {
            groups.wait(settlement);
            assert_eq!(groups.next_group(), None, "a second acknowledgement was issued while one was in flight");
        }
        groups.settle();
        assert_eq!(groups.next_group(), Some(vec![2, 3, 4, 5, 6]));
        assert_eq!(groups.next_group(), None);
    }

    #[test]
    fn an_acknowledgement_that_comes_back_with_nothing_waiting_issues_nothing_further() {
        let mut groups = AcknowledgementGroups::new();
        groups.wait(1);
        assert_eq!(groups.next_group(), Some(vec![1]));
        groups.settle();
        assert_eq!(groups.next_group(), None::<Vec<i32>>);
    }

    #[test]
    fn nothing_is_acknowledged_before_any_attempt_ends() {
        let mut groups = AcknowledgementGroups::<i32>::new();
        assert_eq!(groups.next_group(), None);
    }
}
