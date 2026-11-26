//! Sending one claimed delivery and deciding how the queue settles it.
//!
//! The verdict is the delivery policy's: the attempt ceilings, the retry
//! wait and the bound on a stored diagnostic are applied here, so the queue
//! records a verdict as given and never needs to know any of them. The wait
//! is handed over as a step the queue adds to its own clock, never as an
//! instant read from the broker's.
//!
//! A retried failure waits its backoff step, or the wait its endpoint asked
//! for with `Retry-After` (RFC 9110 section 10.2.3, and RFC 6585 section 4
//! for a 429) when that is longer, up to `max_retry_interval`. A failure
//! that exhausts its delivery is not attempted again, so what it asked for
//! is moot.
//!
//! A failure gets the attempts its [`FailureClass`] allows: an unavailable
//! endpoint the full ceiling, since an outage may last, one that answered
//! another server error the lower server-error ceiling, since every
//! notification queued behind it waits, and a permanent failure none.

use crate::{
    delivery_attempt_ceiling::DeliveryAttemptCeiling,
    delivery_backoff::DeliveryBackoff,
    delivery_claim::UnreadableDelivery,
    delivery_entry::DeliveryEntry,
    delivery_error_bound::DeliveryErrorBound,
    delivery_policy::{attempt_ceiling, error_bound, retry_schedule, server_error_ceiling},
    delivery_settlement::DeliveryVerdict,
    delivery_transport::{DeliveryDisposition, NotificationDeliveryTransport},
    error::NotificationError,
    notification_invariant::NotificationInvariant,
};
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_delivery::failure_class::FailureClass;
use std::sync::Arc;
use tracing::{debug, error};

/// Attempts claimed deliveries and judges their outcomes.
pub(crate) struct DeliveryAttempt {
    /// Sends one notification and classifies the endpoint's answer.
    transport: Arc<dyn NotificationDeliveryTransport>,
    /// The longest failure diagnostic the queue stores.
    error_bound: DeliveryErrorBound,
    /// The attempt after which an unavailable endpoint exhausts the delivery.
    attempt_ceiling: DeliveryAttemptCeiling,
    /// The attempt after which a server error exhausts the delivery.
    server_error_ceiling: DeliveryAttemptCeiling,
    /// The wait before a failed delivery is due again.
    retry_schedule: DeliveryBackoff,
}

impl DeliveryAttempt {
    /// Attempt deliveries through `transport` under the policy `config` sets.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Invariant`] when the configured retry
    /// schedule, stored diagnostic bound or an attempt ceiling is out of
    /// range.
    pub(crate) fn new(transport: Arc<dyn NotificationDeliveryTransport>, config: &DeliveryConfig) -> Result<DeliveryAttempt, NotificationError> {
        Ok(DeliveryAttempt {
            transport,
            retry_schedule: retry_schedule(config).map_err(NotificationInvariant::RetrySchedule)?,
            error_bound: error_bound(config).map_err(NotificationInvariant::from)?,
            attempt_ceiling: attempt_ceiling(config).map_err(NotificationInvariant::from)?,
            server_error_ceiling: server_error_ceiling(config).map_err(NotificationInvariant::from)?,
        })
    }

    /// Deliver one claimed entry and settle whatever the transport reported.
    ///
    /// The attempt ceilings are applied here: a failure on the attempt that
    /// reaches the ceiling of its class is settled as exhausted, exactly like
    /// a permanent failure, so the store never has to know a ceiling to keep
    /// an exhausted delivery out of the claimable set.
    ///
    /// A failure that is retried holds the Subscription's queue behind its
    /// delivery of the earliest change still owed, which the store does with
    /// the acknowledgement, and an exhausted probe passes the hold to a
    /// delivery due no sooner than one first retry step later, as a delivery
    /// failing for the first time would be. A deferred delivery keeps its attempts and the hold as
    /// they are, and is due again once its window has passed.
    pub(crate) async fn verdict(&self, entry: &DeliveryEntry) -> Result<DeliveryVerdict, NotificationError> {
        trace_entry(entry);
        match self.transport.deliver(entry).await {
            DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete => Ok(DeliveryVerdict::Delivered),
            DeliveryDisposition::Deferred { window } => Ok(DeliveryVerdict::Deferred {
                retry_after: self.retry_schedule.deferral(window),
            }),
            DeliveryDisposition::Failed {
                class,
                detail,
                retry_after: requested,
            } if self.retries(class, entry.attempt_count) => Ok(DeliveryVerdict::Failed {
                retry_after: self
                    .retry_schedule
                    .step_honouring(entry.attempt_count, requested)
                    .map_err(NotificationInvariant::AttemptCount)?,
                detail: self.error_bound.truncate(&detail).into_owned(),
            }),
            DeliveryDisposition::Failed { detail, .. } => Ok(DeliveryVerdict::Exhausted {
                detail: self.error_bound.truncate(&detail).into_owned(),
                probe_after: self.retry_schedule.first_step(),
            }),
        }
    }

    /// Whether a delivery that had failed `attempts_before` attempts and
    /// failed again with `class` is attempted once more.
    const fn retries(&self, class: FailureClass, attempts_before: i32) -> bool {
        match class {
            FailureClass::Unavailable => !self.attempt_ceiling.is_final_attempt(attempts_before),
            FailureClass::ServerError => !self.server_error_ceiling.is_final_attempt(attempts_before),
            FailureClass::Permanent => false,
        }
    }

    /// The verdict on a claimed delivery whose stored row could not be read
    /// back.
    ///
    /// Nothing a later attempt does can repair a stored value, so the delivery
    /// is settled as exhausted with the reason it could not be read as its
    /// recorded failure, exactly like a delivery whose transport reported a
    /// terminal failure.
    pub(crate) fn unreadable_verdict(&self, unreadable: &UnreadableDelivery) -> DeliveryVerdict {
        error!(
            delivery_key = %unreadable.key,
            error = %unreadable.diagnostic,
            event = "notification_delivery_unreadable"
        );
        DeliveryVerdict::Exhausted {
            detail: self.error_bound.truncate(&unreadable.diagnostic).into_owned(),
            probe_after: self.retry_schedule.first_step(),
        }
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
