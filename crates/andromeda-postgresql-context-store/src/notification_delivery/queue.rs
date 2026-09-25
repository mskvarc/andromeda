//! The queue behind the notification-owned delivery store port.
//!
//! The port speaks one claimed delivery and the event it carries; storage keeps
//! those as two rows joined by the claim. This is the only place the two
//! vocabularies meet: the statements below it know nothing of the port, and the
//! delivery worker above it knows nothing of the join.

use crate::notification_delivery::{acknowledge, claim, cleanup, rows::ClaimedDelivery};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::{
    delivery_entry::{DeliveryEntry, DeliveryId},
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    delivery_store::DeliveryStore,
    error::NotificationError,
    notification_event::{NotificationEvent, NotificationEventId},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use std::fmt::Display;
use tracing::error;

/// One tenant's notification delivery queue.
#[derive(Debug)]
pub struct NotificationDeliveryQueue {
    /// The tenant connection pool.
    pool: PgPool,
}

impl NotificationDeliveryQueue {
    /// Open the delivery queue held in the given tenant's database.
    #[must_use]
    pub fn new(pool: &PgPool) -> NotificationDeliveryQueue {
        NotificationDeliveryQueue { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }
}

#[async_trait]
impl DeliveryStore for NotificationDeliveryQueue {
    async fn claim_pending(&self, worker_id: &LeaseHolder, lease: LeaseTerm, batch_size: i32) -> Result<Vec<DeliveryEntry>, NotificationError> {
        let claimed = claim::claim_due(&self.pool, *worker_id, lease, batch_size)
            .await
            .map_err(|error| NotificationError::persistence(error.to_string()))?;
        Ok(claimed.into_iter().filter_map(readable).collect())
    }

    async fn mark_delivered(&self, id: DeliveryId, worker_id: &LeaseHolder, target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
        let outcome = acknowledge::mark_delivered(&self.pool, id.into_inner(), *worker_id, target)
            .await
            .map_err(|error| DeliveryLeaseError::persistence(error.to_string()))?;
        held(outcome, id, worker_id)
    }

    async fn mark_failed(
        &self,
        id: DeliveryId,
        worker_id: &LeaseHolder,
        failure: &str,
        next_attempt_at: DateTime<Utc>,
        target: &ContextTarget,
    ) -> Result<(), DeliveryLeaseError> {
        let outcome = acknowledge::mark_failed(&self.pool, id.into_inner(), *worker_id, failure, next_attempt_at, target)
            .await
            .map_err(|error| DeliveryLeaseError::persistence(error.to_string()))?;
        held(outcome, id, worker_id)
    }

    async fn mark_terminal(&self, id: DeliveryId, worker_id: &LeaseHolder, failure: &str, target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
        let outcome = acknowledge::mark_exhausted(&self.pool, id.into_inner(), *worker_id, failure, target)
            .await
            .map_err(|error| DeliveryLeaseError::persistence(error.to_string()))?;
        held(outcome, id, worker_id)
    }

    async fn delete_delivered_before(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        cleanup::purge_delivered_before(&self.pool, cutoff)
            .await
            .map_err(|error| NotificationError::persistence(error.to_string()))
    }

    async fn delete_exhausted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        cleanup::purge_exhausted_before(&self.pool, cutoff)
            .await
            .map_err(|error| NotificationError::persistence(error.to_string()))
    }

    async fn delete_orphan_events_before(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        cleanup::purge_orphan_events_before(&self.pool, cutoff)
            .await
            .map_err(|error| NotificationError::persistence(error.to_string()))
    }
}

/// Report a lost lease as the typed failure the port defines.
const fn held(outcome: LeaseAcknowledgement, id: DeliveryId, worker_id: &LeaseHolder) -> Result<(), DeliveryLeaseError> {
    match outcome {
        LeaseAcknowledgement::Applied => Ok(()),
        LeaseAcknowledgement::Lost => Err(DeliveryLeaseError::lease_lost(id, worker_id)),
    }
}

/// Keep a validated delivery and log an unreadable one without blocking its peers.
///
/// The delivery stays claimed until its lease expires and is then retried,
/// which is the same treatment one that failed in flight receives: nothing here
/// can repair a stored value, and refusing the whole batch over one of them
/// would stall every other subscriber.
fn readable(claimed: Result<ClaimedDelivery, impl Display>) -> Option<DeliveryEntry> {
    match claimed {
        Ok(delivery) => Some(entry(delivery)),
        Err(error) => {
            error!(error = %error, event = "notification_delivery_unreadable");
            None
        }
    }
}

/// Project one validated delivery into the value the port hands the worker.
fn entry(delivery: ClaimedDelivery) -> DeliveryEntry {
    DeliveryEntry::builder()
        .target(delivery.target)
        .id(DeliveryId::from(delivery.id))
        .subscription_id(delivery.subscription_id)
        .attempt_count(delivery.attempt_count)
        .next_attempt_at(delivery.next_attempt_at)
        .event(
            NotificationEvent::builder()
                .id(NotificationEventId::from(delivery.event_id))
                .write_id(delivery.write_id)
                .event_ordinal(delivery.event_ordinal)
                .trigger(delivery.trigger)
                .entity_id(delivery.entity_id)
                .changed_attributes(delivery.changed_attributes)
                .changed_dataset_id(delivery.changed_dataset_id)
                .entity_snapshot(delivery.entity_snapshot)
                .previous_snapshot(delivery.previous_snapshot)
                .created_at(delivery.created_at)
                .build(),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        queue::{entry, held},
        rows::ClaimedDelivery,
    };
    use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use andromeda_notification::{delivery_entry::DeliveryId, delivery_lease::DeliveryLeaseError, notification_event::NotificationEventId};
    use chrono::Utc;
    use uuid::Uuid;

    fn delivery() -> ClaimedDelivery {
        ClaimedDelivery {
            target: ContextTarget::Live,
            id: 11,
            subscription_id: "urn:ngsi-ld:Subscription:1".parse().expect("a valid subscription URI"),
            attempt_count: 1,
            next_attempt_at: Utc::now(),
            event_id: 3,
            write_id: WriteId::from_uuid(Uuid::nil()),
            event_ordinal: WriteOrdinal::at(2),
            trigger: NotificationTrigger::EntityCreated,
            entity_id: "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"),
            changed_attributes: Vec::new(),
            changed_dataset_id: None,
            entity_snapshot: None,
            previous_snapshot: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn an_applied_acknowledgement_leaves_the_worker_holding_its_lease() {
        let worker = LeaseHolder::from(Uuid::nil());
        assert!(held(LeaseAcknowledgement::Applied, DeliveryId::from(11_i64), &worker).is_ok());
    }

    #[test]
    fn a_lost_acknowledgement_becomes_the_typed_lost_lease_failure() {
        let worker = LeaseHolder::from(Uuid::nil());
        let outcome = held(LeaseAcknowledgement::Lost, DeliveryId::from(11_i64), &worker);
        assert!(matches!(outcome, Err(DeliveryLeaseError::LeaseLost { .. })));
    }

    #[test]
    fn a_claimed_delivery_becomes_the_entry_the_worker_delivers() {
        let entry = entry(delivery());
        pretty_assertions::assert_eq!(entry.id, DeliveryId::from(11_i64));
        pretty_assertions::assert_eq!(entry.subscription_id.as_str(), "urn:ngsi-ld:Subscription:1");
        pretty_assertions::assert_eq!(entry.attempt_count, 1);
        pretty_assertions::assert_eq!(entry.event.id, NotificationEventId::from(3_i64));
        pretty_assertions::assert_eq!(entry.event.event_ordinal, WriteOrdinal::at(2));
        pretty_assertions::assert_eq!(entry.event.trigger, NotificationTrigger::EntityCreated);
    }
}
