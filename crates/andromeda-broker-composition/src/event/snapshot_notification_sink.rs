//! Delivery binding for snapshot status notifications.

use andromeda_delivery::{
    dispatch::{DeliveryResult, TransportDispatcher},
    endpoint::transport::EndpointTransport,
    notification_context_placement::notification_context_placement,
    request::DeliveryRequest,
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use andromeda_notification::{delivery_backoff::backoff_seconds, delivery_config::DeliveryConfig};
use andromeda_postgresql_context_store::snapshot::status_outbox::{SnapshotDeliveryFailure, SnapshotStatusDelivery, SnapshotStatusOutbox};
use andromeda_snapshot::error::SnapshotError;
use chrono::{DateTime, Duration, Utc};
use futures::{StreamExt, stream};
use std::sync::Arc;
use uuid::Uuid;

/// Delivers snapshot status notifications through the configured transport dispatcher.
pub struct DeliverySnapshotNotificationSink {
    transport: Arc<TransportDispatcher>,
}

/// Bounded, leased delivery pass over one tenant's Snapshot status outbox.
pub struct SnapshotStatusDeliveryDrain {
    outbox: Arc<SnapshotStatusOutbox>,
    transport: Arc<DeliverySnapshotNotificationSink>,
    worker: Uuid,
    lease: Duration,
    max_attempts: i32,
    base_backoff_secs: i64,
    batch_size: i32,
    concurrency: usize,
    error_max_chars: usize,
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
            worker: Uuid::new_v4(),
            lease: Duration::seconds(i64::from(config.lease_secs)),
            max_attempts: config.max_attempts.max(1),
            base_backoff_secs: i64::try_from(config.base_backoff_secs)
                .map_err(|error| SnapshotError::internal(format!("Snapshot notification backoff is out of range: {error}")))?,
            batch_size: config.batch_size.max(1),
            concurrency: usize::try_from(config.concurrency).unwrap_or(usize::MAX).max(1),
            error_max_chars: usize::try_from(config.error_max_chars).unwrap_or(usize::MAX),
            delivered_retention: Duration::seconds(i64::try_from(config.cleanup_after_secs).unwrap_or(i64::MAX)),
            dead_retention: Duration::seconds(i64::try_from(config.dead_letter_after_secs).unwrap_or(i64::MAX)),
        })
    }

    /// Claim, deliver, acknowledge, and clean one bounded batch.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<usize, SnapshotError> {
        let lease_until = now
            .checked_add_signed(self.lease)
            .ok_or_else(|| SnapshotError::internal("Snapshot notification lease deadline overflowed"))?;
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
        let delivered_before = now.checked_sub_signed(self.delivered_retention).unwrap_or(DateTime::<Utc>::MIN_UTC);
        let exhausted_before = now.checked_sub_signed(self.dead_retention).unwrap_or(DateTime::<Utc>::MIN_UTC);
        self.outbox.cleanup(delivered_before, exhausted_before).await?;
        Ok(claimed)
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
                let detail = detail.chars().take(self.error_max_chars).collect::<String>();
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
