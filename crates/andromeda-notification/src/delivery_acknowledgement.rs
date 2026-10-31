//! Recording the outcome of a delivery the worker leased.
//!
//! An outcome the store failed to record leaves the delivery leased and
//! unacknowledged, so once the lease lapses another worker claims it and
//! attempts it again: a notification the endpoint already accepted would be
//! sent a second time, which clause 5.8.6 does not owe the subscriber. A
//! transient store failure (an exhausted pool, a dropped connection) is
//! therefore asked again while the lease can still hold. The store's lease
//! guard ends the retry by itself once the lease lapses, answering that the
//! lease was lost; the bound on requests only stops a worker from asking a
//! store that keeps failing for longer than any lease it holds could last.

use crate::{
    delivery_entry::DeliveryKey,
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    delivery_store::DeliveryStore,
    error::NotificationError,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_retry::{growth::Growth, retry_delay::RetryDelay, retry_number::RetryNumber, retry_policy::RetryPolicy, schedule::BackoffSchedule};
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use chrono::{DateTime, Utc};
use std::{iter::successors, num::NonZeroU32, sync::Arc, time::Duration};
use tracing::warn;

/// The wait before each further request to record an outcome: a tenth of a
/// second, doubling up to two seconds.
const ACKNOWLEDGEMENT_BACKOFF: BackoffSchedule = BackoffSchedule::new(Duration::from_millis(100), Duration::from_secs(2), Growth::Doubling);

/// The worker's channel for recording what became of each delivery it leased.
pub(crate) struct DeliveryAcknowledgement {
    /// The queue the outcomes are recorded in.
    store: Arc<dyn DeliveryStore>,
    /// The worker every lease being acknowledged was taken by.
    worker_id: LeaseHolder,
    /// The timer waited on between requests to record one outcome.
    delay: Arc<dyn RetryDelay>,
    /// How often one outcome is asked for, as many times as the lease term
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

    /// Record the delivery `key` of the branch `target` delivered.
    pub(crate) async fn delivered(&self, key: DeliveryKey, target: &ContextTarget) -> Result<LeaseAcknowledgement, NotificationError> {
        self.record(key, QueueOutcome::Completed, async || {
            self.store.mark_delivered(key, &self.worker_id, target).await
        })
        .await
    }

    /// Record a failed attempt at the delivery `key` of the branch `target`,
    /// due again at `next_attempt_at`, with its diagnostic `detail`.
    pub(crate) async fn failed(
        &self,
        key: DeliveryKey,
        detail: &str,
        next_attempt_at: DateTime<Utc>,
        target: &ContextTarget,
    ) -> Result<LeaseAcknowledgement, NotificationError> {
        self.record(key, QueueOutcome::Retried, async || {
            self.store.mark_failed(key, &self.worker_id, detail, next_attempt_at, target).await
        })
        .await
    }

    /// Record the delivery `key` of the branch `target` exhausted, with its
    /// diagnostic `detail`.
    pub(crate) async fn exhausted(&self, key: DeliveryKey, detail: &str, target: &ContextTarget) -> Result<LeaseAcknowledgement, NotificationError> {
        self.record(key, QueueOutcome::Exhausted, async || {
            self.store.mark_terminal(key, &self.worker_id, detail, target).await
        })
        .await
    }

    /// Record an outcome of the delivery `key` through `write`, asking again
    /// after a transient failure as often as the lease leaves room for, and
    /// count it as `outcome` once the store answers.
    ///
    /// A lost lease is the store's answer rather than a failure, and ends the
    /// retry at once; the last failure of a store that fails every request is
    /// surfaced.
    async fn record(
        &self,
        key: DeliveryKey,
        outcome: QueueOutcome,
        mut write: impl AsyncFnMut() -> Result<(), DeliveryLeaseError>,
    ) -> Result<LeaseAcknowledgement, NotificationError> {
        let recorded = self
            .retry
            .run(
                self.delay.as_ref(),
                |error, retry| match error {
                    DeliveryLeaseError::Persistence(failure) => {
                        warn!(
                            delivery_id = %key,
                            worker_id = %self.worker_id,
                            retry = retry.get(),
                            error = %failure,
                            event = "notification_delivery_acknowledgement_retried"
                        );
                        true
                    }
                    DeliveryLeaseError::LeaseLost { .. } => false,
                },
                async |_| write().await,
            )
            .await;
        let acknowledgement = classify_acknowledgement(&recorded)?;
        record_queue_outcome(
            QueueName::NotificationDeliveries,
            match acknowledgement {
                LeaseAcknowledgement::Applied => outcome,
                LeaseAcknowledgement::Lost => QueueOutcome::LeaseLost,
            },
            1,
        );
        Ok(acknowledgement)
    }
}

/// Absorb a lost lease and surface a failed statement.
///
/// A lost lease is not this pass's failure: the delivery was reclaimed by
/// another worker, which is now responsible for sending it, so the pass records
/// the loss and moves on. A failed statement is a persistence failure the task
/// supervisor must see.
fn classify_acknowledgement(result: &Result<(), DeliveryLeaseError>) -> Result<LeaseAcknowledgement, NotificationError> {
    match result {
        Ok(()) => Ok(LeaseAcknowledgement::Applied),
        Err(DeliveryLeaseError::LeaseLost { delivery, worker }) => {
            warn!(delivery_id = %delivery, worker_id = %worker, event = "notification_delivery_lease_lost");
            Ok(LeaseAcknowledgement::Lost)
        }
        Err(DeliveryLeaseError::Persistence(failure)) => Err(NotificationError::Persistence(*failure)),
    }
}

/// The retry recording one outcome runs under, for a delivery leased for
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
