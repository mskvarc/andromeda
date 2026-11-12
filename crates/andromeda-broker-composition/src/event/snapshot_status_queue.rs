//! The leased queue the Snapshot status drain claims its deliveries from.

use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_notification::delivery_backoff::BackoffStep;
use andromeda_postgresql_context_store::snapshot::status_outbox::{SnapshotDeliveryFailure, SnapshotStatusDelivery, SnapshotStatusOutbox};
use andromeda_snapshot::error::SnapshotError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{num::NonZeroU32, time::Duration};

/// A durable queue of Snapshot status deliveries whose rows a worker holds by lease.
///
/// Every statement that acts on a claimed delivery reports whether the worker
/// still held it, so a worker whose lease lapsed and was taken over learns that
/// the delivery is no longer its own.
///
/// Leases and retry waits are handed over as spans, which the queue counts
/// from its own clock, the clock its claims compare with.
#[async_trait]
pub(crate) trait SnapshotStatusQueue: Send + Sync + 'static {
    /// Claim up to `limit` due deliveries, each leased for `lease` from the claim.
    async fn claim(&self, worker: LeaseHolder, lease: Duration, limit: NonZeroU32) -> Result<Vec<SnapshotStatusDelivery>, SnapshotError>;

    /// Extend the lease on one claimed delivery to `lease` from the renewal.
    async fn renew(&self, delivery: &SnapshotStatusDelivery, worker: LeaseHolder, lease: Duration) -> Result<LeaseAcknowledgement, SnapshotError>;

    /// Mark one claimed delivery delivered.
    async fn acknowledge(&self, delivery: &SnapshotStatusDelivery, worker: LeaseHolder) -> Result<LeaseAcknowledgement, SnapshotError>;

    /// Record a failed attempt on one claimed delivery, rescheduling it
    /// `retry_after` from the failure or dead-lettering it.
    async fn fail(
        &self,
        delivery: &SnapshotStatusDelivery,
        worker: LeaseHolder,
        failure: SnapshotDeliveryFailure,
        max_attempts: i32,
        retry_after: BackoffStep,
        detail: &str,
    ) -> Result<LeaseAcknowledgement, SnapshotError>;

    /// Remove delivered rows older than `delivered_before` and dead-lettered
    /// rows older than `exhausted_before`.
    async fn cleanup(&self, delivered_before: DateTime<Utc>, exhausted_before: DateTime<Utc>) -> Result<u64, SnapshotError>;
}

#[async_trait]
impl SnapshotStatusQueue for SnapshotStatusOutbox {
    async fn claim(&self, worker: LeaseHolder, lease: Duration, limit: NonZeroU32) -> Result<Vec<SnapshotStatusDelivery>, SnapshotError> {
        SnapshotStatusOutbox::claim(self, worker, lease, limit).await
    }

    async fn renew(&self, delivery: &SnapshotStatusDelivery, worker: LeaseHolder, lease: Duration) -> Result<LeaseAcknowledgement, SnapshotError> {
        SnapshotStatusOutbox::renew(self, delivery.id, worker, lease).await
    }

    async fn acknowledge(&self, delivery: &SnapshotStatusDelivery, worker: LeaseHolder) -> Result<LeaseAcknowledgement, SnapshotError> {
        SnapshotStatusOutbox::acknowledge(self, delivery.id, worker).await
    }

    async fn fail(
        &self,
        delivery: &SnapshotStatusDelivery,
        worker: LeaseHolder,
        failure: SnapshotDeliveryFailure,
        max_attempts: i32,
        retry_after: BackoffStep,
        detail: &str,
    ) -> Result<LeaseAcknowledgement, SnapshotError> {
        SnapshotStatusOutbox::fail(self, delivery.id, worker, failure, max_attempts, retry_after, detail).await
    }

    async fn cleanup(&self, delivered_before: DateTime<Utc>, exhausted_before: DateTime<Utc>) -> Result<u64, SnapshotError> {
        SnapshotStatusOutbox::cleanup(self, delivered_before, exhausted_before).await
    }
}
