//! The queue behind the notification-owned delivery store port.
//!
//! The port speaks one claimed delivery and the event it carries; storage keeps
//! those as two rows joined by the claim. This is the only place the two
//! vocabularies meet: the statements below it know nothing of the port, and the
//! delivery worker above it knows nothing of the join.

use crate::{
    notification_delivery::{
        acknowledge,
        claim,
        cleanup,
        rows::{ClaimedDelivery, UnreadableClaimedDelivery},
    },
    store_fault::persistence_failure,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::{
    delivery_claim::{DeliveryClaim, UnreadableDelivery},
    delivery_entry::{DeliveryEntry, DeliveryKey},
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    delivery_store::DeliveryStore,
    error::NotificationError,
    notification_event::{NotificationEvent, NotificationEventKey},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use std::sync::Arc;

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
    async fn claim_pending(&self, worker_id: &LeaseHolder, lease: LeaseTerm, batch_size: i32) -> Result<Vec<DeliveryClaim>, NotificationError> {
        let claimed = claim::claim_due(&self.pool, *worker_id, lease, batch_size)
            .await
            .map_err(|error| NotificationError::Persistence(persistence_failure(&error)))?;
        Ok(claimed.into_iter().map(delivery_claim).collect())
    }

    async fn mark_delivered(&self, key: DeliveryKey, worker_id: &LeaseHolder, target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
        let outcome = acknowledge::mark_delivered(&self.pool, key.into_inner(), *worker_id, target)
            .await
            .map_err(|error| DeliveryLeaseError::Persistence(persistence_failure(&error)))?;
        held(outcome, key, worker_id)
    }

    async fn mark_failed(
        &self,
        key: DeliveryKey,
        worker_id: &LeaseHolder,
        failure: &str,
        next_attempt_at: DateTime<Utc>,
        target: &ContextTarget,
    ) -> Result<(), DeliveryLeaseError> {
        let outcome = acknowledge::mark_failed(&self.pool, key.into_inner(), *worker_id, failure, next_attempt_at, target)
            .await
            .map_err(|error| DeliveryLeaseError::Persistence(persistence_failure(&error)))?;
        held(outcome, key, worker_id)
    }

    async fn mark_terminal(&self, key: DeliveryKey, worker_id: &LeaseHolder, failure: &str, target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
        let outcome = acknowledge::mark_exhausted(&self.pool, key.into_inner(), *worker_id, failure, target)
            .await
            .map_err(|error| DeliveryLeaseError::Persistence(persistence_failure(&error)))?;
        held(outcome, key, worker_id)
    }

    async fn delete_delivered_before(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        cleanup::purge_delivered_before(&self.pool, cutoff)
            .await
            .map_err(|error| NotificationError::Persistence(persistence_failure(&error)))
    }

    async fn delete_exhausted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        cleanup::purge_exhausted_before(&self.pool, cutoff)
            .await
            .map_err(|error| NotificationError::Persistence(persistence_failure(&error)))
    }

    async fn delete_orphan_events_before(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        cleanup::purge_orphan_events_before(&self.pool, cutoff)
            .await
            .map_err(|error| NotificationError::Persistence(persistence_failure(&error)))
    }
}

/// Report a lost lease as the typed failure the port defines.
const fn held(outcome: LeaseAcknowledgement, key: DeliveryKey, worker_id: &LeaseHolder) -> Result<(), DeliveryLeaseError> {
    match outcome {
        LeaseAcknowledgement::Applied => Ok(()),
        LeaseAcknowledgement::Lost => Err(DeliveryLeaseError::lease_lost(key, worker_id)),
    }
}

/// Hand a claimed row to the worker as the delivery it reads back as, or as the
/// leased row it could not be read back as.
///
/// Nothing here can repair a stored value, and refusing the whole batch over
/// one of them would stall every other subscriber; the worker acknowledges the
/// unreadable row instead, so it leaves the claimable set rather than being
/// claimed again at every lease expiry.
fn delivery_claim(claimed: Result<ClaimedDelivery, Box<UnreadableClaimedDelivery>>) -> DeliveryClaim {
    match claimed {
        Ok(delivery) => DeliveryClaim::Readable(Box::new(entry(delivery))),
        Err(unreadable) => {
            let diagnostic = unreadable.to_string();
            DeliveryClaim::Unreadable(UnreadableDelivery {
                target: unreadable.target,
                key: unreadable.delivery_key,
                diagnostic,
            })
        }
    }
}

/// Project one validated delivery into the value the port hands the worker.
fn entry(delivery: ClaimedDelivery) -> DeliveryEntry {
    DeliveryEntry::builder()
        .target(delivery.target)
        .key(delivery.delivery_key)
        .subscription_id(delivery.subscription_id)
        .attempt_count(delivery.attempt_count)
        .next_attempt_at(delivery.next_attempt_at)
        .event(
            NotificationEvent::builder()
                .key(NotificationEventKey::from(delivery.event_key))
                .write_id(delivery.write_id)
                .event_ordinal(delivery.event_ordinal)
                .change(delivery.change)
                .entity_id(delivery.entity_id)
                .entity_snapshot(delivery.entity_snapshot.map(Arc::new))
                .previous_snapshot(delivery.previous_snapshot.map(Arc::new))
                .created_at(delivery.created_at)
                .build(),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        queue::{delivery_claim, entry, held},
        rows::{ClaimedDelivery, ClaimedDeliveryError, UnreadableClaimedDelivery},
    };
    use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use andromeda_notification::{
        delivery_claim::DeliveryClaim,
        delivery_entry::DeliveryKey,
        delivery_lease::DeliveryLeaseError,
        notification_event::NotificationEventKey,
    };
    use chrono::Utc;
    use uuid::Uuid;

    fn delivery() -> ClaimedDelivery {
        ClaimedDelivery {
            target: ContextTarget::Live,
            delivery_key: DeliveryKey::from(11_i64),
            subscription_id: "urn:ngsi-ld:Subscription:1".parse().expect("a valid subscription URI"),
            attempt_count: 1,
            next_attempt_at: Utc::now(),
            event_key: 3,
            write_id: WriteId::from_uuid(Uuid::nil()),
            event_ordinal: WriteOrdinal::at(2),
            change: EntityChange::new(EntityLifecycle::Created, Vec::new()),
            entity_id: "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"),
            entity_snapshot: None,
            previous_snapshot: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn an_applied_acknowledgement_leaves_the_worker_holding_its_lease() {
        let worker = LeaseHolder::from(Uuid::nil());
        assert!(held(LeaseAcknowledgement::Applied, DeliveryKey::from(11_i64), &worker).is_ok());
    }

    #[test]
    fn a_lost_acknowledgement_becomes_the_typed_lost_lease_failure() {
        let worker = LeaseHolder::from(Uuid::nil());
        let outcome = held(LeaseAcknowledgement::Lost, DeliveryKey::from(11_i64), &worker);
        assert!(matches!(outcome, Err(DeliveryLeaseError::LeaseLost { .. })));
    }

    #[test]
    fn a_claimed_delivery_becomes_the_entry_the_worker_delivers() {
        let entry = entry(delivery());
        pretty_assertions::assert_eq!(entry.key, DeliveryKey::from(11_i64));
        pretty_assertions::assert_eq!(entry.subscription_id.as_str(), "urn:ngsi-ld:Subscription:1");
        pretty_assertions::assert_eq!(entry.attempt_count, 1);
        pretty_assertions::assert_eq!(entry.event.key, NotificationEventKey::from(3_i64));
        pretty_assertions::assert_eq!(entry.event.event_ordinal, WriteOrdinal::at(2));
        pretty_assertions::assert_eq!(entry.event.change.lifecycle, EntityLifecycle::Created);
    }

    /// An unreadable row is still leased to the worker, so it reaches the
    /// worker with the key, branch and reason its acknowledgement records
    /// rather than being dropped from the claim.
    #[test]
    fn an_unreadable_claimed_row_reaches_the_worker_to_be_acknowledged() {
        let unreadable = Box::new(UnreadableClaimedDelivery {
            target: ContextTarget::Live,
            delivery_key: DeliveryKey::from(11_i64),
            reason: ClaimedDeliveryError::InvalidEventOrdinal {
                source: WriteOrdinal::try_from(-1).expect_err("a negative ordinal"),
            },
        });
        match delivery_claim(Err(unreadable)) {
            DeliveryClaim::Unreadable(claim) => {
                pretty_assertions::assert_eq!(claim.key, DeliveryKey::from(11_i64));
                pretty_assertions::assert_eq!(claim.target, ContextTarget::Live);
                pretty_assertions::assert_eq!(claim.diagnostic, "notification delivery 11 carries an unreadable event ordinal");
            }
            DeliveryClaim::Readable(entry) => panic!("an unreadable row must not be handed over as deliverable: {entry:?}"),
        }
    }

    #[test]
    fn a_readable_claimed_row_reaches_the_worker_as_its_delivery() {
        match delivery_claim(Ok(delivery())) {
            DeliveryClaim::Readable(entry) => pretty_assertions::assert_eq!(entry.key, DeliveryKey::from(11_i64)),
            DeliveryClaim::Unreadable(claim) => panic!("a readable row must be deliverable: {claim:?}"),
        }
    }
}
