//! Recording notification outcomes on their subscription with one write in
//! flight per subscription.
//!
//! Every outcome of one subscription updates the same delivery-state row, so
//! writes issued one per outcome wait on that row's lock one after another,
//! and the subscription is recorded no faster than one write at a time however
//! many of its notifications are in flight. Outcomes are therefore queued per
//! subscription and branch: while a write is in flight for it, the outcomes
//! that arrive wait, and the next write records all of them as one tally as
//! soon as the first returns. An outcome arriving with no write in flight is
//! written at once, with no timer.
//!
//! Each caller still waits for the write that carries its outcome, so an
//! outcome is recorded before its delivery is acknowledged, at most two writes
//! after it arrives.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::{
    error::SubscriptionError,
    notification_slot::{NotificationOutcome, OutcomeRecord},
    outcome_tally::OutcomeTally,
    store::SubscriptionStore,
};
use batch_aint_one::{BatchError, Batcher, BatchingPolicy, Limits, Processor};
use std::{future::ready, sync::Arc};

/// The most outcomes one write adds up.
const MOST_OUTCOMES_PER_WRITE: usize = 4_096;

/// The most outcomes of one subscription that wait for its write in flight;
/// a further one is refused and not recorded.
const MOST_OUTCOMES_QUEUED: usize = 16_384;

/// The subscription, on its branch, whose delivery-state row an outcome moves.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OutcomeOwner {
    /// The branch the subscription is stored on.
    target: ContextTarget,
    /// The subscription notified.
    subscription: Identifier,
}

/// The write adding up one subscription's queued outcomes.
#[derive(Clone)]
struct DeliveryStateWrite {
    /// The store holding the delivery-state rows.
    store: Arc<dyn SubscriptionStore>,
}

impl Processor for DeliveryStateWrite {
    type Key = OutcomeOwner;
    type Input = (NotificationOutcome, OutcomeRecord);
    type Output = ();
    type Error = Arc<SubscriptionError>;
    type Resources = ();

    fn acquire_resources(&self, _key: OutcomeOwner) -> impl Future<Output = Result<(), Arc<SubscriptionError>>> + Send {
        ready(Ok(()))
    }

    async fn process(
        &self,
        key: OutcomeOwner,
        inputs: impl Iterator<Item = (NotificationOutcome, OutcomeRecord)> + Send,
        (): (),
    ) -> Result<Vec<()>, Arc<SubscriptionError>> {
        let mut inputs = inputs;
        let Some((outcome, record)) = inputs.next() else {
            return Ok(Vec::new());
        };
        let (tally, count) = inputs.fold((OutcomeTally::new(outcome, record), 1_usize), |(mut tally, count), (outcome, record)| {
            tally.include(outcome, record);
            (tally, count.saturating_add(1))
        });
        self.store.record_outcomes(&key.target, &key.subscription, tally).await.map_err(Arc::new)?;
        Ok(vec![(); count])
    }
}

/// Why a notification outcome was not recorded.
#[derive(Debug, thiserror::Error)]
pub(crate) enum OutcomeNotRecorded {
    /// The write carrying it failed.
    #[error("the delivery-state write failed: {0}")]
    Write(Arc<SubscriptionError>),
    /// It never reached a write: its subscription's queue was full, or the
    /// writer has stopped.
    #[error("the outcome was not queued for a write: {0}")]
    Queue(BatchError<Arc<SubscriptionError>>),
}

/// Records notification outcomes, one write in flight per subscription.
pub(crate) struct OutcomeBatcher {
    /// The per-subscription queues and the writes draining them.
    batcher: Batcher<DeliveryStateWrite>,
}

impl OutcomeBatcher {
    /// A batcher writing to `store`; it runs a worker task on the current
    /// Tokio runtime until it is dropped.
    pub(crate) fn new(store: Arc<dyn SubscriptionStore>) -> OutcomeBatcher {
        let limits = Limits::builder()
            .max_batch_size(MOST_OUTCOMES_PER_WRITE)
            .max_key_concurrency(1)
            .max_key_queued_items(MOST_OUTCOMES_QUEUED)
            .build();
        OutcomeBatcher {
            batcher: Batcher::builder()
                .name("notification-outcomes")
                .processor(DeliveryStateWrite { store })
                .limits(limits)
                .batching_policy(BatchingPolicy::Immediate)
                .build(),
        }
    }

    /// Record the `outcome` of one notification to `subscription` on
    /// `target`, with what `record` adds to it, once the write carrying it
    /// returns.
    pub(crate) async fn record(
        &self,
        target: &ContextTarget,
        subscription: &Identifier,
        outcome: NotificationOutcome,
        record: OutcomeRecord,
    ) -> Result<(), OutcomeNotRecorded> {
        let owner = OutcomeOwner {
            target: target.clone(),
            subscription: subscription.clone(),
        };
        self.batcher.add(owner, (outcome, record)).await.map_err(|error| match error.inner() {
            Ok(store) => OutcomeNotRecorded::Write(store),
            Err(queue) => OutcomeNotRecorded::Queue(queue),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        outcome_batching::{OutcomeBatcher, OutcomeNotRecorded},
        outcome_recording_store::{OutcomeRecordingStore, WriteAnswer},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_subscription::{
        notification_slot::{NotificationOutcome, OutcomeRecord},
        outcome_tally::OutcomeTally,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use futures::future::join_all;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use tokio::task::yield_now;

    fn subscription(name: &str) -> Identifier {
        format!("urn:ngsi-ld:Subscription:{name}").parse().expect("a valid identifier")
    }

    fn counted_at(seconds: i64) -> OutcomeRecord {
        let notified_at: DateTime<Utc> = Utc.timestamp_opt(seconds, 0).single().expect("a valid instant");
        OutcomeRecord::CountingNotification { notified_at }
    }

    /// Every third outcome of the later ones fails.
    fn later_outcome(seconds: i64) -> (NotificationOutcome, OutcomeRecord) {
        let outcome = if seconds % 3 == 0 {
            NotificationOutcome::Failure
        } else {
            NotificationOutcome::Success
        };
        (outcome, counted_at(seconds))
    }

    /// Yield until the callers, the batcher's worker and the writes it started
    /// have each run as far as they can on the current-thread runtime.
    async fn settle() {
        for _ in 0..64 {
            yield_now().await;
        }
    }

    #[tokio::test]
    async fn a_lone_outcome_is_written_at_once_as_a_tally_of_one() {
        let store = Arc::new(OutcomeRecordingStore::new(None, WriteAnswer::Written));
        let batcher = OutcomeBatcher::new(Arc::<OutcomeRecordingStore>::clone(&store));
        let sub = subscription("lone");
        batcher
            .record(&ContextTarget::Live, &sub, NotificationOutcome::Success, counted_at(10))
            .await
            .expect("the outcome is recorded");
        assert_eq!(store.writes(), vec![(sub, OutcomeTally::new(NotificationOutcome::Success, counted_at(10)))]);
    }

    /// The nine outcomes arrive while the first write holds the row, so the
    /// next write carries all nine and none of them waits for a write of its
    /// own.
    #[tokio::test]
    async fn outcomes_arriving_during_a_write_are_recorded_together_by_the_next_write() {
        let sub = subscription("busy");
        let store = Arc::new(OutcomeRecordingStore::new(Some(sub.clone()), WriteAnswer::Written));
        let batcher = OutcomeBatcher::new(Arc::<OutcomeRecordingStore>::clone(&store));
        let first = batcher.record(&ContextTarget::Live, &sub, NotificationOutcome::Success, counted_at(10));
        let later = async {
            store.held_write_started().await;
            let records = join_all((11..20).map(|seconds| {
                let (outcome, record) = later_outcome(seconds);
                batcher.record(&ContextTarget::Live, &sub, outcome, record)
            }));
            let release = async {
                settle().await;
                store.release(2);
            };
            tokio::join!(records, release).0
        };
        let (first, later) = tokio::join!(first, later);
        first.expect("the first outcome is recorded");
        assert!(later.iter().all(Result::is_ok), "{later:?}");
        let mut nine = OutcomeTally::new(NotificationOutcome::Success, counted_at(11));
        (12..20).map(later_outcome).for_each(|(outcome, record)| nine.include(outcome, record));
        assert_eq!(
            store.writes(),
            vec![(sub.clone(), OutcomeTally::new(NotificationOutcome::Success, counted_at(10))), (sub, nine)]
        );
    }

    #[tokio::test]
    async fn a_write_in_flight_for_one_subscription_does_not_hold_another_subscriptions_outcome() {
        let busy = subscription("busy");
        let other = subscription("other");
        let store = Arc::new(OutcomeRecordingStore::new(Some(busy.clone()), WriteAnswer::Written));
        let batcher = OutcomeBatcher::new(Arc::<OutcomeRecordingStore>::clone(&store));
        let held = batcher.record(&ContextTarget::Live, &busy, NotificationOutcome::Success, counted_at(10));
        let meanwhile = async {
            store.held_write_started().await;
            let recorded = batcher
                .record(&ContextTarget::Live, &other, NotificationOutcome::Failure, OutcomeRecord::OutcomeOnly)
                .await;
            let writes = store.writes();
            store.release(1);
            (recorded, writes)
        };
        let (held, (recorded, writes_while_held)) = tokio::join!(held, meanwhile);
        held.expect("the held outcome is recorded once released");
        recorded.expect("the other subscription's outcome is recorded while the first write is held");
        assert_eq!(
            writes_while_held,
            vec![
                (busy, OutcomeTally::new(NotificationOutcome::Success, counted_at(10))),
                (other, OutcomeTally::new(NotificationOutcome::Failure, OutcomeRecord::OutcomeOnly))
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_write_is_reported_to_every_outcome_it_carried() {
        let sub = subscription("failing");
        let store = Arc::new(OutcomeRecordingStore::new(Some(sub.clone()), WriteAnswer::Saturated));
        let batcher = OutcomeBatcher::new(Arc::<OutcomeRecordingStore>::clone(&store));
        let first = batcher.record(&ContextTarget::Live, &sub, NotificationOutcome::Success, counted_at(10));
        let later = async {
            store.held_write_started().await;
            let records = join_all((11..14).map(|seconds| {
                let (outcome, record) = later_outcome(seconds);
                batcher.record(&ContextTarget::Live, &sub, outcome, record)
            }));
            let release = async {
                settle().await;
                store.release(2);
            };
            tokio::join!(records, release).0
        };
        let (first, later) = tokio::join!(first, later);
        assert!(matches!(first, Err(OutcomeNotRecorded::Write(_))), "{first:?}");
        assert!(later.iter().all(|result| matches!(result, Err(OutcomeNotRecorded::Write(_)))), "{later:?}");
        assert_eq!(store.writes().len(), 2);
    }
}
