//! Recording the outcomes of the deliveries the worker leased, beside the
//! claim that refills their slots.
//!
//! A round the store failed to record leaves its deliveries leased and
//! unacknowledged, so once a lease lapses another worker claims the delivery
//! and attempts it again: a notification the endpoint already accepted would
//! be sent a second time, which clause 5.8.6 does not owe the subscriber. A
//! transient store failure (an exhausted pool, a dropped connection) is
//! therefore asked again while the leases can still hold. The store records a
//! round whole or not at all, so asking again records no delivery twice and
//! claims nothing twice, and its lease guard ends the retry by itself once a
//! lease lapses, answering that the lease was lost; the bound on requests
//! only stops a worker from asking a store that keeps failing for longer than
//! any lease it holds could last.

use crate::{
    delivery_lease::LeaseTerm,
    delivery_round::{ClaimRequest, SettledRound},
    delivery_settlement::{DeliverySettlement, DeliveryVerdict},
    delivery_store::DeliveryStore,
    error::NotificationError,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_retry::{growth::Growth, retry_delay::RetryDelay, retry_number::RetryNumber, retry_policy::RetryPolicy, schedule::BackoffSchedule};
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use std::{iter::successors, num::NonZeroU32, sync::Arc, time::Duration};
use tracing::warn;

/// The wait before each further request to record a round: a tenth of a
/// second, doubling up to two seconds.
const ACKNOWLEDGEMENT_BACKOFF: BackoffSchedule = BackoffSchedule::new(Duration::from_millis(100), Duration::from_secs(2), Growth::Doubling);

/// The worker's channel for recording what became of the deliveries it
/// leased.
pub(crate) struct DeliveryAcknowledgement {
    /// The queue the outcomes are recorded in.
    store: Arc<dyn DeliveryStore>,
    /// The worker every lease being acknowledged was taken by.
    worker_id: LeaseHolder,
    /// The timer waited on between requests to record one round.
    delay: Arc<dyn RetryDelay>,
    /// How often one round is asked for, as many times as the lease term
    /// leaves room for.
    retry: RetryPolicy,
}

impl DeliveryAcknowledgement {
    /// Record outcomes in `store` for the leases `worker_id` takes for
    /// `lease`, waiting on `delay` between requests.
    pub(crate) fn new(store: Arc<dyn DeliveryStore>, worker_id: LeaseHolder, delay: Arc<dyn RetryDelay>, lease: LeaseTerm) -> DeliveryAcknowledgement {
        DeliveryAcknowledgement {
            store,
            worker_id,
            delay,
            retry: acknowledgement_retry(lease),
        }
    }

    /// Record `settlements` as one group and claim what `refill` asks for,
    /// asking again with both after a transient failure as often as the lease
    /// leaves room for, and count each settlement once the store answers.
    ///
    /// The answers name one per settlement, in order. A lost lease is the
    /// store's answer rather than a failure: the delivery was reclaimed by
    /// another worker, which is now responsible for sending it, so the pass
    /// records the loss and moves on. The last failure of a store that fails
    /// every request is surfaced, for the task supervisor to see.
    pub(crate) async fn settle_and_claim(
        &self,
        settlements: &[DeliverySettlement],
        refill: Option<ClaimRequest<'_>>,
    ) -> Result<SettledRound, NotificationError> {
        let round = self
            .retry
            .run(
                self.delay.as_ref(),
                |error, retry| match error {
                    NotificationError::Persistence(failure) => {
                        warn!(
                            deliveries = settlements.len(),
                            worker_id = %self.worker_id,
                            retry = retry.get(),
                            error = %failure,
                            event = "notification_delivery_acknowledgement_retried"
                        );
                        true
                    }
                    NotificationError::Entity(_)
                    | NotificationError::Subscription(_)
                    | NotificationError::Registration(_)
                    | NotificationError::JsonLd(_)
                    | NotificationError::Compaction(_)
                    | NotificationError::Representation(_)
                    | NotificationError::Rendering(_)
                    | NotificationError::Invariant(_) => false,
                },
                async |_| self.store.settle_and_claim(&self.worker_id, settlements, refill).await,
            )
            .await?;
        self.count(settlements, &round.answers);
        Ok(round)
    }

    /// Report each lost lease and count every settlement under the queue
    /// outcome it reached.
    fn count(&self, settlements: &[DeliverySettlement], answers: &[LeaseAcknowledgement]) {
        for (settlement, answer) in settlements.iter().zip(answers) {
            let outcome = match answer {
                LeaseAcknowledgement::Applied => match settlement.verdict {
                    DeliveryVerdict::Delivered => QueueOutcome::Completed,
                    DeliveryVerdict::Failed { .. } => QueueOutcome::Retried,
                    DeliveryVerdict::Exhausted { .. } => QueueOutcome::Exhausted,
                },
                LeaseAcknowledgement::Lost => {
                    warn!(delivery_id = %settlement.key, worker_id = %self.worker_id, event = "notification_delivery_lease_lost");
                    QueueOutcome::LeaseLost
                }
            };
            record_queue_outcome(QueueName::NotificationDeliveries, outcome, 1);
        }
    }
}

/// The retry recording one round runs under, for deliveries leased for
/// `lease`: the first request, and one more after each backoff wait that still
/// ends within the term.
pub(crate) fn acknowledgement_retry(lease: LeaseTerm) -> RetryPolicy {
    let term = Duration::from_secs(u64::from(lease.seconds()));
    let retries = successors(Some(RetryNumber::FIRST), |retry| Some(retry.next()))
        .scan(Duration::ZERO, |waited, retry| {
            *waited = waited.saturating_add(ACKNOWLEDGEMENT_BACKOFF.delay(retry));
            Some(*waited)
        })
        .take_while(|waited| *waited <= term)
        .count();
    RetryPolicy {
        attempts: u32::try_from(retries).map_or(NonZeroU32::MAX, |retries| NonZeroU32::MIN.saturating_add(retries)),
        schedule: ACKNOWLEDGEMENT_BACKOFF,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{delivery_acknowledgement::acknowledgement_retry, delivery_lease::LeaseTerm};
    use andromeda_retry::retry_number::RetryNumber;
    use std::{iter::successors, time::Duration};

    fn lease(seconds: u32) -> LeaseTerm {
        LeaseTerm::from_seconds(seconds).expect("a valid lease term")
    }

    /// Waits of 0.1, 0.2, 0.4, 0.8 and 1.6 seconds, then thirteen of two,
    /// add up to 29.1 seconds; a fourteenth would end past the 30-second
    /// lease. Eighteen retries after the first request make nineteen.
    #[test]
    fn a_thirty_second_lease_is_asked_nineteen_times() {
        pretty_assertions::assert_eq!(acknowledgement_retry(lease(30)).attempts.get(), 19);
    }

    /// Waits of 0.1, 0.2 and 0.4 seconds end within one second; the next, of
    /// 0.8, would end at 1.5.
    #[test]
    fn a_one_second_lease_is_asked_four_times() {
        pretty_assertions::assert_eq!(acknowledgement_retry(lease(1)).attempts.get(), 4);
    }

    #[test]
    fn every_wait_of_the_retry_ends_within_the_lease() {
        let policy = acknowledgement_retry(lease(30));
        let waited: Duration = successors(Some(RetryNumber::FIRST), |retry| Some(retry.next()))
            .take(usize::try_from(policy.attempts.get() - 1).expect("a representable count"))
            .map(|retry| policy.schedule.delay(retry))
            .sum();
        assert!(waited <= Duration::from_secs(30), "{waited:?}");
    }
}
