//! Sending one claimed delivery and deciding how the queue settles it.
//!
//! The verdict is the delivery policy's: the attempt ceiling, the retry
//! wait and the bound on a stored diagnostic are applied here, so the queue
//! records a verdict as given and never needs to know any of them.

use crate::{
    delivery_attempt_ceiling::DeliveryAttemptCeiling,
    delivery_backoff::retry_due_at,
    delivery_claim::UnreadableDelivery,
    delivery_entry::DeliveryEntry,
    delivery_error_bound::DeliveryErrorBound,
    delivery_policy::{attempt_ceiling, error_bound, retry_schedule},
    delivery_settlement::DeliveryVerdict,
    delivery_transport::{DeliveryDisposition, NotificationDeliveryTransport},
    error::NotificationError,
    notification_invariant::NotificationInvariant,
};
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_context_commit::lease_pass_clock::LeasePassClock;
use andromeda_retry::schedule::BackoffSchedule;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::{debug, error};

/// Attempts claimed deliveries and judges their outcomes.
pub(crate) struct DeliveryAttempt {
    /// Sends one notification and classifies the endpoint's answer.
    transport: Arc<dyn NotificationDeliveryTransport>,
    /// The longest failure diagnostic the queue stores.
    error_bound: DeliveryErrorBound,
    /// The attempt after which a retryable failure exhausts the delivery.
    attempt_ceiling: DeliveryAttemptCeiling,
    /// The wait before a failed delivery is due again.
    retry_schedule: BackoffSchedule,
}

impl DeliveryAttempt {
    /// Attempt deliveries through `transport` under the policy `config` sets.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Invariant`] when the configured retry
    /// schedule, stored diagnostic bound or attempt ceiling is out of range.
    pub(crate) fn new(transport: Arc<dyn NotificationDeliveryTransport>, config: &DeliveryConfig) -> Result<DeliveryAttempt, NotificationError> {
        Ok(DeliveryAttempt {
            transport,
            retry_schedule: retry_schedule(config).map_err(NotificationInvariant::RetrySchedule)?,
            error_bound: error_bound(config).map_err(NotificationInvariant::from)?,
            attempt_ceiling: attempt_ceiling(config).map_err(NotificationInvariant::from)?,
        })
    }

    /// Deliver one claimed entry and settle whatever the transport reported.
    ///
    /// The attempt ceiling is applied here: a retryable failure on the attempt
    /// that reaches the ceiling is settled as exhausted, exactly like a
    /// terminal failure, so the store never has to know the ceiling to keep an
    /// exhausted delivery out of the claimable set.
    ///
    /// A retryable failure holds the rest of the Subscription's queue behind
    /// the failed delivery, which the store does with the acknowledgement, and
    /// an exhausted probe hands the hold to a delivery due one first retry step
    /// later, as a delivery failing for the first time would be.
    pub(crate) async fn verdict(&self, entry: &DeliveryEntry, clock: &LeasePassClock) -> Result<DeliveryVerdict, NotificationError> {
        trace_entry(entry);
        match self.transport.deliver(entry).await {
            DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete => Ok(DeliveryVerdict::Delivered),
            DeliveryDisposition::RetryableFailure { detail } if !self.attempt_ceiling.is_final_attempt(entry.attempt_count) => Ok(DeliveryVerdict::Failed {
                next_attempt_at: retry_due_at(&self.retry_schedule, entry.attempt_count, clock.now()).map_err(NotificationInvariant::AttemptCount)?,
                detail: self.error_bound.truncate(&detail).into_owned(),
            }),
            DeliveryDisposition::RetryableFailure { detail } | DeliveryDisposition::TerminalFailure { detail } => Ok(DeliveryVerdict::Exhausted {
                detail: self.error_bound.truncate(&detail).into_owned(),
                next_probe_at: self.next_probe_at(clock)?,
            }),
        }
    }

    /// The instant the next probe of a held Subscription is due when the
    /// delivery being exhausted was its probe: one first retry step from now.
    fn next_probe_at(&self, clock: &LeasePassClock) -> Result<DateTime<Utc>, NotificationError> {
        Ok(retry_due_at(&self.retry_schedule, 0, clock.now()).map_err(NotificationInvariant::AttemptCount)?)
    }

    /// The verdict on a claimed delivery whose stored row could not be read
    /// back.
    ///
    /// Nothing a later attempt does can repair a stored value, so the delivery
    /// is settled as exhausted with the reason it could not be read as its
    /// recorded failure, exactly like a delivery whose transport reported a
    /// terminal failure.
    pub(crate) fn unreadable_verdict(&self, unreadable: &UnreadableDelivery, clock: &LeasePassClock) -> Result<DeliveryVerdict, NotificationError> {
        error!(
            delivery_key = %unreadable.key,
            error = %unreadable.diagnostic,
            event = "notification_delivery_unreadable"
        );
        Ok(DeliveryVerdict::Exhausted {
            detail: self.error_bound.truncate(&unreadable.diagnostic).into_owned(),
            next_probe_at: self.next_probe_at(clock)?,
        })
    }
}

/// Record what a claimed delivery carries before it is attempted.
fn trace_entry(entry: &DeliveryEntry) {
    debug!(
        delivery_key = %entry.key,
        event_key = %entry.event.key,
        subscription_id = %entry.subscription_id,
        entity_id = %entry.event.entity_id,
        lifecycle = ?entry.event.change.lifecycle,
        attempt = entry.attempt_count,
        has_snapshot = entry.event.entity_snapshot.is_some(),
        has_previous = entry.event.previous_snapshot.is_some(),
        changed_attributes = ?entry.event.change.attributes(),
        created_at = %entry.event.created_at,
        next_attempt_at = %entry.next_attempt_at,
        "Processing notification delivery"
    );
}
