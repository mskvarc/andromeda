//! Delivery binding for snapshot status notifications.

use crate::event::snapshot_status_queue::SnapshotStatusQueue;
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, lease_pass_clock::LeasePassClock};
use andromeda_delivery::{
    dispatch::{DeliveryResult, TransportDispatcher},
    endpoint::transport::EndpointTransport,
    notification_body::NotificationBody,
    notification_context_placement::notification_context_placement,
    request::DeliveryRequest,
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_notification::{
    delivery_backoff::retry_due_at,
    delivery_error_bound::DeliveryErrorBound,
    delivery_policy::{error_bound, retry_schedule},
};
use andromeda_postgresql_context_store::snapshot::status_outbox::{SnapshotDeliveryFailure, SnapshotStatusDelivery};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_snapshot::{error::SnapshotError, snapshot_invariant::SnapshotInvariant};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use futures::future::join_all;
use std::{num::NonZeroU32, sync::Arc};

/// Delivers snapshot status notifications through the configured transport dispatcher.
pub struct DeliverySnapshotNotificationSink {
    transport: Arc<TransportDispatcher>,
}

/// Sends one leased Snapshot status notification.
#[async_trait]
pub trait SnapshotStatusTransport: Send + Sync + 'static {
    /// Send the notification of one claimed delivery and classify the answer.
    async fn deliver_status(&self, delivery: &SnapshotStatusDelivery) -> DeliveryResult;
}

/// What one pass over the Snapshot status outbox did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotStatusPassOutcome {
    /// Number of deliveries claimed during the pass.
    pub claimed: usize,
    /// Number of claimed deliveries another worker owned by the time this one
    /// acted on them, so this pass sent nothing for them or recorded no outcome.
    pub lease_lost: usize,
}

/// Bounded, leased delivery pass over one tenant's Snapshot status outbox.
pub struct SnapshotStatusDeliveryDrain {
    outbox: Arc<dyn SnapshotStatusQueue>,
    transport: Arc<dyn SnapshotStatusTransport>,
    worker: LeaseHolder,
    lease: Duration,
    max_attempts: i32,
    /// The wait before a failed status notification is due again.
    retry_schedule: BackoffSchedule,
    /// Deliveries one pass claims and sends at once at most:
    /// `notifications.delivery.max_concurrent`.
    claim_width: NonZeroU32,
    error_bound: DeliveryErrorBound,
    delivered_retention: Duration,
    dead_retention: Duration,
}

impl SnapshotStatusDeliveryDrain {
    /// Construct a drain from the same durable-delivery policy subscriptions use.
    pub fn new(
        outbox: Arc<dyn SnapshotStatusQueue>,
        transport: Arc<dyn SnapshotStatusTransport>,
        config: &DeliveryConfig,
    ) -> Result<SnapshotStatusDeliveryDrain, SnapshotError> {
        Ok(SnapshotStatusDeliveryDrain {
            outbox,
            transport,
            worker: LeaseHolder::mint(),
            lease: Duration::seconds(i64::try_from(config.queue.lease_timeout.duration().as_secs()).unwrap_or(i64::MAX)),
            max_attempts: i32::try_from(config.max_attempts.get()).unwrap_or(i32::MAX),
            retry_schedule: retry_schedule(config).map_err(|error| {
                tracing::error!(%error, "the configured Snapshot notification retry schedule is unusable");
                SnapshotInvariant::NotificationRetrySchedule
            })?,
            claim_width: config.max_concurrent,
            error_bound: error_bound(config).map_err(|error| {
                tracing::error!(%error, "the configured Snapshot notification error bound is unusable");
                SnapshotInvariant::NotificationErrorBound
            })?,
            delivered_retention: Duration::seconds(i64::try_from(config.retention.acknowledged_after.duration().as_secs()).unwrap_or(i64::MAX)),
            dead_retention: Duration::seconds(i64::try_from(config.retention.dead_letter_after.duration().as_secs()).unwrap_or(i64::MAX)),
        })
    }

    /// Claim, deliver and acknowledge up to `max_concurrent` deliveries, all
    /// sent at once.
    ///
    /// Each delivery renews its lease right before it is sent, and
    /// `request_timeout` plus the lease margin the configuration enforces
    /// keeps that lease alive through the attempt. A delivery whose renewal
    /// finds another worker holding it is not sent, because the worker that
    /// reclaimed it sends it.
    ///
    /// Retention cleanup is a separate concern on its own cadence
    /// ([`cleanup`](Self::cleanup)), so a wake-driven delivery pass issues no
    /// `DELETE`: it does only the work its wake announced.
    ///
    /// # Errors
    ///
    /// Returns the first [`SnapshotError`] the claim, a renewal or an
    /// acknowledgement reports, after every claimed delivery has been settled.
    /// A lease another worker took over is counted in
    /// [`SnapshotStatusPassOutcome::lease_lost`] rather than surfaced as an error.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<SnapshotStatusPassOutcome, SnapshotError> {
        let clock = LeasePassClock::starting_at(now);
        let entries = self.outbox.claim(self.worker, self.lease_deadline(clock.now())?, self.claim_width).await?;
        let claimed = entries.len();
        let results = join_all(entries.into_iter().map(|entry| self.deliver_one(entry, &clock))).await;
        let mut lease_lost = 0;
        for result in results {
            if result? == LeaseAcknowledgement::Lost {
                lease_lost += 1;
            }
        }
        Ok(SnapshotStatusPassOutcome { claimed, lease_lost })
    }

    /// The deliveries one [`execute`](Self::execute) pass claims at most.
    ///
    /// A pass that claimed that many may have more waiting, which is what
    /// lets the drain driver run another pass immediately rather than sleep.
    #[must_use]
    pub const fn claim_width(&self) -> NonZeroU32 {
        self.claim_width
    }

    /// Remove acknowledged and exhausted deliveries past their retention.
    ///
    /// Runs on its own cadence rather than inside a delivery pass, so widening a
    /// tenant's idle delivery interval does not also stop it reclaiming rows.
    pub async fn cleanup(&self, now: DateTime<Utc>) -> Result<(), SnapshotError> {
        let delivered_before = now.checked_sub_signed(self.delivered_retention).unwrap_or(DateTime::<Utc>::MIN_UTC);
        let exhausted_before = now.checked_sub_signed(self.dead_retention).unwrap_or(DateTime::<Utc>::MIN_UTC);
        self.outbox.cleanup(delivered_before, exhausted_before).await?;
        Ok(())
    }

    /// The instant a lease taken at `from` runs out.
    fn lease_deadline(&self, from: DateTime<Utc>) -> Result<DateTime<Utc>, SnapshotError> {
        Ok(from.checked_add_signed(self.lease).ok_or(SnapshotInvariant::LeaseDeadlineOverflow)?)
    }

    /// Renew one claimed delivery's lease, send it, and record the outcome.
    ///
    /// The outcome is [`LeaseAcknowledgement::Lost`] when another worker owned
    /// the delivery at the renewal, which sends nothing, or at the recording of
    /// its outcome.
    async fn deliver_one(&self, entry: SnapshotStatusDelivery, clock: &LeasePassClock) -> Result<LeaseAcknowledgement, SnapshotError> {
        let renewal = self.outbox.renew(&entry, self.worker, self.lease_deadline(clock.now())?).await?;
        if renewal == LeaseAcknowledgement::Lost {
            Self::report_lease_lost(&entry);
            return Ok(LeaseAcknowledgement::Lost);
        }
        let acknowledgement = match self.transport.deliver_status(&entry).await {
            DeliveryResult::Delivered => self.outbox.acknowledge(&entry, self.worker).await?,
            failure @ (DeliveryResult::RetryableFailure | DeliveryResult::TerminalFailure) => {
                let retryable = failure.should_retry();
                let failure = if retryable {
                    SnapshotDeliveryFailure::Retryable
                } else {
                    SnapshotDeliveryFailure::Terminal
                };
                let retry_at = retry_due_at(&self.retry_schedule, entry.attempt_count, clock.now()).map_err(SnapshotInvariant::NotificationAttemptCount)?;
                let detail = if retryable {
                    "Snapshot notification delivery failed transiently"
                } else {
                    "Snapshot notification delivery failed terminally"
                };
                let detail = self.error_bound.truncate(detail);
                self.outbox.fail(&entry, self.worker, failure, self.max_attempts, retry_at, &detail).await?
            }
        };
        if acknowledgement == LeaseAcknowledgement::Lost {
            Self::report_lease_lost(&entry);
        }
        Ok(acknowledgement)
    }

    /// Log a delivery that another worker took over.
    fn report_lease_lost(entry: &SnapshotStatusDelivery) {
        tracing::warn!(
            snapshot_id = %entry.snapshot_id,
            outbox_id = %entry.id,
            event = "snapshot_status_delivery_lease_lost",
            "another worker took over a Snapshot status delivery"
        );
    }
}

impl DeliverySnapshotNotificationSink {
    /// Construct the notification binding.
    pub const fn new(transport: Arc<TransportDispatcher>) -> DeliverySnapshotNotificationSink {
        DeliverySnapshotNotificationSink { transport }
    }
}

#[async_trait]
impl SnapshotStatusTransport for DeliverySnapshotNotificationSink {
    /// Deliver one durably leased Snapshot lifecycle event over HTTP.
    async fn deliver_status(&self, delivery: &SnapshotStatusDelivery) -> DeliveryResult {
        let request = DeliveryRequest {
            // clone: the leased outbox row remains available to record the result after transport completes
            body: NotificationBody::new(delivery.notification.clone()),
            endpoint_uri: delivery.endpoint.clone(),
            content_type: ResponseMediaType::JsonLd,
            // The notification was written with its `@context` when the event
            // committed; clause 6.3.8 is asked where it belongs rather than the
            // answer being restated here.
            context_placement: notification_context_placement(ResponseMediaType::JsonLd, delivery.receiver_info.as_deref()),
            context_url: None,
            tenant: None,
            snapshot: None,
            receiver_info: delivery.receiver_info.clone().unwrap_or_default(),
            // A Snapshot status callback is addressed by a Snapshot's own
            // notification target, not by the clause 5.2.15 `Endpoint` of a
            // Subscription, so there is no per-endpoint `timeout` to honour and
            // the broker-wide deadline applies.
            timeout: None,
            subscription_id: delivery.snapshot_id.clone(),
        };
        self.transport.deliver(&EndpointTransport::Http, &request).await
    }
}

#[cfg(test)]
mod tests;
