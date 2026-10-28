//! The leased queue the Snapshot status drain claims its deliveries from.

use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_postgresql_context_store::snapshot::status_outbox::{SnapshotDeliveryFailure, SnapshotStatusDelivery, SnapshotStatusOutbox};
use andromeda_snapshot::error::SnapshotError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::num::NonZeroU32;

/// A durable queue of Snapshot status deliveries whose rows a worker holds by lease.
///
/// Every statement that acts on a claimed delivery reports whether the worker
/// still held it, so a worker whose lease lapsed and was taken over learns that
/// the delivery is no longer its own.
#[async_trait]
pub trait SnapshotStatusQueue: Send + Sync + 'static {
    /// Claim up to `limit` due deliveries under a lease that runs to `lease_until`.
    async fn claim(&self, worker: LeaseHolder, lease_until: DateTime<Utc>, limit: NonZeroU32) -> Result<Vec<SnapshotStatusDelivery>, SnapshotError>;

    /// Extend the lease on one claimed delivery to `lease_until`.
    async fn renew(&self, delivery: &SnapshotStatusDelivery, worker: LeaseHolder, lease_until: DateTime<Utc>) -> Result<LeaseAcknowledgement, SnapshotError>;

    /// Mark one claimed delivery delivered.
    async fn acknowledge(&self, delivery: &SnapshotStatusDelivery, worker: LeaseHolder) -> Result<LeaseAcknowledgement, SnapshotError>;

    /// Record a failed attempt on one claimed delivery, rescheduling it for
    /// `retry_at` or dead-lettering it.
    async fn fail(
        &self,
        delivery: &SnapshotStatusDelivery,
        worker: LeaseHolder,
        failure: SnapshotDeliveryFailure,
        max_attempts: i32,
        retry_at: DateTime<Utc>,
        detail: &str,
    ) -> Result<LeaseAcknowledgement, SnapshotError>;

    /// Remove delivered rows older than `delivered_before` and dead-lettered
    /// rows older than `exhausted_before`.
    async fn cleanup(&self, delivered_before: DateTime<Utc>, exhausted_before: DateTime<Utc>) -> Result<u64, SnapshotError>;
}

#[async_trait]
impl SnapshotStatusQueue for SnapshotStatusOutbox {
    async fn claim(&self, worker: LeaseHolder, lease_until: DateTime<Utc>, limit: NonZeroU32) -> Result<Vec<SnapshotStatusDelivery>, SnapshotError> {
        SnapshotStatusOutbox::claim(self, worker, lease_until, limit).await
    }

    async fn renew(&self, delivery: &SnapshotStatusDelivery, worker: LeaseHolder, lease_until: DateTime<Utc>) -> Result<LeaseAcknowledgement, SnapshotError> {
        SnapshotStatusOutbox::renew(self, delivery.id, worker, lease_until).await
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
        retry_at: DateTime<Utc>,
        detail: &str,
    ) -> Result<LeaseAcknowledgement, SnapshotError> {
        SnapshotStatusOutbox::fail(self, delivery.id, worker, failure, max_attempts, retry_at, detail).await
    }

    async fn cleanup(&self, delivered_before: DateTime<Utc>, exhausted_before: DateTime<Utc>) -> Result<u64, SnapshotError> {
        SnapshotStatusOutbox::cleanup(self, delivered_before, exhausted_before).await
    }
}
