//! Delivery binding for snapshot status notifications.

use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_delivery::{
    dispatch::{DeliveryResult, TransportDispatcher},
    endpoint::transport::EndpointTransport,
    notification_context_placement::notification_context_placement,
    request::DeliveryRequest,
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use andromeda_notification::{delivery_backoff::backoff_seconds, delivery_config::DeliveryConfig, delivery_error_bound::DeliveryErrorBound};
use andromeda_postgresql_context_store::snapshot::status_outbox::{SnapshotDeliveryFailure, SnapshotStatusDelivery, SnapshotStatusOutbox};
use andromeda_snapshot::{error::SnapshotError, snapshot_invariant::SnapshotInvariant};
use chrono::{DateTime, Duration, Utc};
use futures::{StreamExt, stream};
use std::sync::Arc;

/// Delivers snapshot status notifications through the configured transport dispatcher.
pub struct DeliverySnapshotNotificationSink {
    transport: Arc<TransportDispatcher>,
}

/// Bounded, leased delivery pass over one tenant's Snapshot status outbox.
pub struct SnapshotStatusDeliveryDrain {
    outbox: Arc<SnapshotStatusOutbox>,
    transport: Arc<DeliverySnapshotNotificationSink>,
    worker: LeaseHolder,
    lease: Duration,
    max_attempts: i32,
    base_backoff_secs: i64,
    batch_size: i32,
    concurrency: usize,
    error_bound: DeliveryErrorBound,
    delivered_retention: Duration,
    dead_retention: Duration,
}

impl SnapshotStatusDeliveryDrain {
    /// Construct a drain from the same durable-delivery policy subscriptions use.
    pub fn new(
        outbox: Arc<SnapshotStatusOutbox>,
        transport: Arc<DeliverySnapshotNotificationSink>,
        config: &DeliveryConfig,
    ) -> Result<SnapshotStatusDeliveryDrain, SnapshotError> {
        Ok(SnapshotStatusDeliveryDrain {
            outbox,
            transport,
            worker: LeaseHolder::mint(),
            lease: Duration::seconds(i64::try_from(config.lease.duration().as_secs()).unwrap_or(i64::MAX)),
            max_attempts: config.max_attempts.max(1),
            base_backoff_secs: i64::try_from(config.base_backoff.duration().as_secs()).map_err(SnapshotInvariant::NotificationBackoff)?,
            batch_size: config.batch_size.max(1),
            concurrency: usize::try_from(config.concurrency).unwrap_or(usize::MAX).max(1),
            error_bound: config.error_bound().map_err(|error| {
                tracing::error!(%error, "the configured Snapshot notification error bound is unusable");
                SnapshotInvariant::NotificationErrorBound
            })?,
            delivered_retention: Duration::seconds(i64::try_from(config.cleanup_after.duration().as_secs()).unwrap_or(i64::MAX)),
            dead_retention: Duration::seconds(i64::try_from(config.dead_letter_after.duration().as_secs()).unwrap_or(i64::MAX)),
        })
    }

    /// Claim, deliver and acknowledge one bounded batch, reporting how many were
    /// claimed.
    ///
    /// Retention cleanup is a separate concern on its own cadence
    /// ([`cleanup`](Self::cleanup)), so a wake-driven delivery pass issues no
    /// `DELETE`: it does only the work its wake announced.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<usize, SnapshotError> {
        let lease_until = now.checked_add_signed(self.lease).ok_or(SnapshotInvariant::LeaseDeadlineOverflow)?;
        let entries = self.outbox.claim(self.worker, lease_until, self.batch_size).await?;
        let claimed = entries.len();
        let results = stream::iter(entries)
            .map(|entry| async move { self.deliver_one(entry, now).await })
            .buffer_unordered(self.concurrency)
            .collect::<Vec<_>>()
            .await;
        for result in results {
            result?;
        }
        Ok(claimed)
    }

    /// The batch size one [`execute`](Self::execute) pass claims at most.
    ///
    /// A pass that claimed a full batch may have more waiting, which is what
    /// lets the drain driver run another pass immediately rather than sleep.
    #[must_use]
    pub const fn batch_size(&self) -> i32 {
        self.batch_size
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

    async fn deliver_one(&self, entry: SnapshotStatusDelivery, now: DateTime<Utc>) -> Result<(), SnapshotError> {
        let outcome = self.transport.deliver_status(&entry).await;
        match outcome {
            DeliveryResult::Delivered => {
                self.outbox.acknowledge(entry.id, self.worker).await?;
            }
            failure @ (DeliveryResult::RetryableFailure | DeliveryResult::TerminalFailure) => {
                let retryable = failure.should_retry();
                let failure = if retryable {
                    SnapshotDeliveryFailure::Retryable
                } else {
                    SnapshotDeliveryFailure::Terminal
                };
                let backoff = backoff_seconds(self.base_backoff_secs, entry.attempt_count, self.max_attempts);
                let retry_at = now.checked_add_signed(Duration::seconds(backoff)).unwrap_or(DateTime::<Utc>::MAX_UTC);
                let detail = if retryable {
                    "Snapshot notification delivery failed transiently"
                } else {
                    "Snapshot notification delivery failed terminally"
                };
                let detail = self.error_bound.truncate(detail);
                self.outbox.fail(entry.id, self.worker, failure, self.max_attempts, retry_at, &detail).await?;
            }
        }
        Ok(())
    }
}

impl DeliverySnapshotNotificationSink {
    /// Construct the notification binding.
    pub const fn new(transport: Arc<TransportDispatcher>) -> DeliverySnapshotNotificationSink {
        DeliverySnapshotNotificationSink { transport }
    }

    /// Deliver one durably leased Snapshot lifecycle event over HTTP.
    pub async fn deliver_status(&self, delivery: &SnapshotStatusDelivery) -> DeliveryResult {
        // clone: the leased outbox row remains available to record the result after transport completes
        let mut payload = delivery.payload.clone();
        if let Some(document) = payload.as_object_mut() {
            document.insert("@context".to_owned(), serde_json::Value::String(NGSI_LD_CORE_CONTEXT_URL.to_string()));
        }
        let request = DeliveryRequest {
            payload: Arc::new(payload),
            endpoint_uri: delivery.endpoint.clone(),
            content_type: ResponseMediaType::JsonLd,
            // The `@context` was just inserted above; clause 6.3.8 is asked
            // where it belongs rather than the answer being restated here.
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
mod tests {
    #[test]
    fn a_delivery_pass_issues_no_cleanup() -> Result<(), &'static str> {
        // The drain is database-backed, so this guards the split structurally:
        // the delivery pass is defined before the cleanup pass, and the only
        // outbox cleanup call sits inside the cleanup pass, so a wake-driven
        // delivery pass issues no retention DELETE.
        let source = include_str!("snapshot_notification_sink.rs");
        let execute = source.find("pub async fn execute").ok_or("delivery pass missing")?;
        let cleanup_fn = source.find("pub async fn cleanup").ok_or("cleanup pass missing")?;
        let cleanup_call = source.find("self.outbox.cleanup(").ok_or("outbox cleanup call missing")?;
        assert!(execute < cleanup_fn, "the delivery pass is defined before the cleanup pass");
        assert!(cleanup_call > cleanup_fn, "the outbox cleanup is issued only from the cleanup pass");
        Ok(())
    }
}
