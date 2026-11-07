//! The order in which the reads of one cached subscription take effect.
//!
//! A subscription is read again when it changes, when a durably queued
//! delivery addressed to it is drained, and by the reconcile pass while its
//! newest read could not reach the store. The reads run concurrently and finish
//! in any order, so each takes a ticket before it is sent, and its outcome takes
//! effect only when no read with a later ticket has taken effect first. A read
//! sent after a change committed sees that change, so the outcome in effect
//! covers every change committed before the newest settled read began, and a
//! slow read can neither overwrite a newer answer nor clear the mark a newer
//! failed read set.
//!
//! A subscription is tracked only while a read of it is pending or its newest
//! read could not reach the store, so nothing is held for one at rest.

use crate::error::SubscriptionError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// The place of one read of a subscription in the order its reads began.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RefreshTicket(u64);

/// What one read of a subscription leaves the cache with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadOutcome {
    /// The store answered, or the change itself said the subscription is gone:
    /// the cache holds the subscription as it now stands.
    Answered,
    /// The store could not be reached: the cache may hold an older version,
    /// and the reconcile pass reads the subscription again.
    Unreachable,
    /// The store failed the read in a way no later read is expected to mend:
    /// the cache keeps what it holds until the subscription changes again.
    Faulted,
}

/// The pending reads of one subscription, and the newest that took effect.
#[derive(Debug, Default)]
struct Reads {
    /// Reads begun and not yet settled or abandoned.
    pending: usize,
    /// The newest read that took effect, with what it left the cache with.
    newest: Option<(RefreshTicket, ReadOutcome)>,
}

impl Reads {
    /// Whether the newest read that took effect could not reach the store.
    const fn is_unreachable(&self) -> bool {
        matches!(self.newest, Some((_, ReadOutcome::Unreachable)))
    }
}

/// The reads of every cached subscription with a read pending or a newest read
/// that could not reach the store.
#[derive(Debug, Default)]
pub(crate) struct RefreshOrder {
    /// The ticket the next read takes. Tickets are compared only between reads
    /// of one subscription, each taken under that subscription's entry, so the
    /// counter needs no ordering of its own.
    next_ticket: AtomicU64,
    /// The reads of each subscription, keyed by its target and identifier.
    subscriptions: DashMap<(ContextTarget, Identifier), Reads>,
}

impl RefreshOrder {
    /// Begin a read of the subscription `id` in `target`, before it is sent.
    pub(crate) fn begin(&self, target: &ContextTarget, id: &Identifier) -> PendingRead<'_> {
        let key = (target.clone(), id.clone()); // clone: the pending read owns the key its place is kept under
        let ticket = {
            let mut reads = self.subscriptions.entry(key.clone()).or_default(); // clone: the order keeps the key while a read is pending
            reads.pending = reads.pending.saturating_add(1);
            RefreshTicket(self.next_ticket.fetch_add(1, Ordering::Relaxed))
        };
        PendingRead { order: self, key, ticket }
    }

    /// The subscriptions whose newest read could not reach the store.
    pub(crate) fn unreachable(&self) -> Vec<(ContextTarget, Identifier)> {
        self.subscriptions
            .iter()
            .filter(|entry| entry.is_unreachable())
            .map(|entry| entry.key().clone()) // clone: the reads run outside the map's shard locks
            .collect()
    }

    /// How many subscriptions [`RefreshOrder::unreachable`] names.
    pub(crate) fn unreachable_len(&self) -> usize {
        self.subscriptions.iter().filter(|entry| entry.is_unreachable()).count()
    }
}

/// A read of one subscription that has begun and not yet settled.
///
/// Dropping it unsettled, as a cancelled read does, gives up its place without
/// an outcome.
#[derive(Debug)]
#[must_use = "a read takes effect only once it is settled"]
pub(crate) struct PendingRead<'order> {
    /// The order the read holds a place in.
    order: &'order RefreshOrder,
    /// The subscription read, by its target and identifier.
    key: (ContextTarget, Identifier),
    /// The read's place among the reads of the subscription.
    ticket: RefreshTicket,
}

impl PendingRead<'_> {
    /// Settle the read with the store's answer, running `apply` to index what
    /// it read when it takes effect.
    pub(crate) fn answered(self, apply: impl FnOnce()) {
        self.settle(ReadOutcome::Answered, apply);
    }

    /// Settle the read with the store's failure: a store that could not be
    /// reached leaves the subscription for the reconcile pass, a fault does not.
    pub(crate) fn failed(self, error: &SubscriptionError) {
        let outcome = if error.is_transient() {
            ReadOutcome::Unreachable
        } else {
            ReadOutcome::Faulted
        };
        self.settle(outcome, || ());
    }

    /// Record `outcome`, and run `apply`, unless a read that began later took
    /// effect first.
    ///
    /// `apply` runs while the subscription's entry is locked, so the outcomes
    /// that take effect are applied in the order their reads began.
    fn settle(self, outcome: ReadOutcome, apply: impl FnOnce()) {
        if let Some(mut reads) = self.order.subscriptions.get_mut(&self.key)
            && reads.newest.is_none_or(|(newest, _)| newest < self.ticket)
        {
            reads.newest = Some((self.ticket, outcome));
            apply();
        }
    }
}

impl Drop for PendingRead<'_> {
    fn drop(&mut self) {
        self.order.subscriptions.remove_if_mut(&self.key, |_, reads| {
            reads.pending = reads.pending.saturating_sub(1);
            reads.pending == 0 && !reads.is_unreachable()
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::SubscriptionError, refresh_order::RefreshOrder};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use serde_json::from_str;
    use std::cell::RefCell;

    fn subscription(name: &str) -> Identifier {
        format!("urn:ngsi-ld:Subscription:{name}").parse().expect("a valid identifier")
    }

    fn unreachable() -> SubscriptionError {
        SubscriptionError::StoreUnavailable
    }

    fn undecodable() -> SubscriptionError {
        SubscriptionError::Representation(from_str::<u8>("not a number").expect_err("text is not a number"))
    }

    #[test]
    fn reads_settling_in_the_order_they_began_all_take_effect() {
        let order = RefreshOrder::default();
        let applied = RefCell::new(Vec::new());
        let first = order.begin(&ContextTarget::Live, &subscription("ordered"));
        let second = order.begin(&ContextTarget::Live, &subscription("ordered"));

        first.answered(|| applied.borrow_mut().push("first"));
        second.answered(|| applied.borrow_mut().push("second"));

        assert_eq!(applied.into_inner(), vec!["first", "second"]);
        assert!(order.subscriptions.is_empty(), "a subscription at rest is not tracked");
    }

    /// The later read was sent after the earlier one, so it saw at least as
    /// new a subscription: the earlier answer is stale once the later one
    /// took effect.
    #[test]
    fn an_answer_that_settles_after_a_later_one_took_effect_is_dropped() {
        let order = RefreshOrder::default();
        let applied = RefCell::new(Vec::new());
        let slow = order.begin(&ContextTarget::Live, &subscription("overtaken"));
        let fast = order.begin(&ContextTarget::Live, &subscription("overtaken"));

        fast.answered(|| applied.borrow_mut().push("fast"));
        slow.answered(|| applied.borrow_mut().push("slow"));

        assert_eq!(applied.into_inner(), vec!["fast"]);
        assert!(order.subscriptions.is_empty());
    }

    /// A reconcile read sent before a change committed answers after the
    /// change's own read could not reach the store: the change is still
    /// unread, so the subscription stays marked.
    #[test]
    fn an_older_answer_neither_takes_effect_nor_clears_a_newer_unreachable_mark() {
        let order = RefreshOrder::default();
        let applied = RefCell::new(Vec::new());
        let reconcile = order.begin(&ContextTarget::Live, &subscription("raced"));
        let change = order.begin(&ContextTarget::Live, &subscription("raced"));

        change.failed(&unreachable());
        reconcile.answered(|| applied.borrow_mut().push("reconcile"));

        assert_eq!(applied.into_inner(), Vec::<&str>::new());
        assert_eq!(order.unreachable(), vec![(ContextTarget::Live, subscription("raced"))]);
    }

    #[test]
    fn an_unreachable_read_keeps_the_subscription_marked_with_no_read_pending() {
        let order = RefreshOrder::default();

        order.begin(&ContextTarget::Live, &subscription("marked")).failed(&unreachable());

        assert_eq!(order.unreachable_len(), 1);
        assert_eq!(order.subscriptions.len(), 1);
    }

    #[test]
    fn an_answer_after_an_unreachable_read_clears_the_mark() {
        let order = RefreshOrder::default();
        order.begin(&ContextTarget::Live, &subscription("recovered")).failed(&unreachable());

        order.begin(&ContextTarget::Live, &subscription("recovered")).answered(|| ());

        assert_eq!(order.unreachable_len(), 0);
        assert!(order.subscriptions.is_empty());
    }

    /// Reading a row that cannot be decoded again decodes nothing, so the
    /// subscription is not read again on every pass.
    #[test]
    fn a_fault_clears_the_mark_and_marks_nothing() {
        let order = RefreshOrder::default();
        order.begin(&ContextTarget::Live, &subscription("faulted")).failed(&unreachable());

        order.begin(&ContextTarget::Live, &subscription("faulted")).failed(&undecodable());
        order.begin(&ContextTarget::Live, &subscription("never-marked")).failed(&undecodable());

        assert_eq!(order.unreachable_len(), 0);
        assert!(order.subscriptions.is_empty());
    }

    #[test]
    fn a_read_dropped_unsettled_gives_up_its_place_and_keeps_an_earlier_mark() {
        let order = RefreshOrder::default();
        drop(order.begin(&ContextTarget::Live, &subscription("cancelled")));
        assert!(order.subscriptions.is_empty());

        order.begin(&ContextTarget::Live, &subscription("cancelled")).failed(&unreachable());
        drop(order.begin(&ContextTarget::Live, &subscription("cancelled")));

        assert_eq!(order.unreachable_len(), 1);
    }

    #[test]
    fn the_reads_of_different_subscriptions_do_not_order_each_other() {
        let order = RefreshOrder::default();
        let applied = RefCell::new(Vec::new());
        let first = order.begin(&ContextTarget::Live, &subscription("first"));
        let second = order.begin(&ContextTarget::Live, &subscription("second"));

        second.answered(|| applied.borrow_mut().push("second"));
        first.answered(|| applied.borrow_mut().push("first"));

        assert_eq!(applied.into_inner(), vec!["second", "first"]);
    }
}
