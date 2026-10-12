//! The queue behind the notification-owned delivery store port.
//!
//! The port speaks one claimed delivery and the event it carries; storage keeps
//! those as two rows, and a claim reads its deliveries and then each event they
//! answer once. The statements below it know nothing of the port, and the
//! delivery worker above it knows nothing of the two reads.

use crate::{
    notification_delivery::{
        acknowledge,
        claim::{self, ClaimDictionaries},
        cleanup,
        rows::UnreadableClaimedDelivery,
    },
    store_fault::persistence_failure,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_notification::{
    delivery_claim::{DeliveryClaim, UnreadableDelivery},
    delivery_entry::{DeliveryEntry, DeliveryKey},
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    delivery_store::DeliveryStore,
    error::NotificationError,
};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// One tenant's notification delivery queue.
#[derive(Debug)]
pub struct NotificationDeliveryQueue {
    /// The tenant connection pool.
    pool: PgPool,
    /// The tenant's `attribute_names` dictionary, which the states a claim
    /// reads hold their attribute names in.
    attribute_names: Arc<dyn NameLookup<AttributeName>>,
    /// The tenant's `entity_type_names` dictionary, which the states a claim
    /// reads hold their types and type IRIs in.
    entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
}

impl NotificationDeliveryQueue {
    /// Open the delivery queue held in the given tenant's database, reading
    /// the names its states hold through the tenant's dictionaries.
    #[must_use]
    pub fn new(
        pool: &PgPool,
        attribute_names: Arc<dyn NameLookup<AttributeName>>,
        entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
    ) -> NotificationDeliveryQueue {
        NotificationDeliveryQueue {
            pool: pool.clone(), // clone: Arc ref-count bump (PgPool is Arc-based)
            attribute_names,
            entity_type_names,
        }
    }
}

#[async_trait]
impl DeliveryStore for NotificationDeliveryQueue {
    async fn claim_pending(&self, worker_id: &LeaseHolder, lease: LeaseTerm, batch_size: i32) -> Result<Vec<DeliveryClaim>, NotificationError> {
        let dictionaries = ClaimDictionaries {
            attribute_names: self.attribute_names.as_ref(),
            entity_type_names: self.entity_type_names.as_ref(),
        };
        let claimed = claim::claim_due(&self.pool, dictionaries, *worker_id, lease, batch_size)
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
fn delivery_claim(claimed: Result<DeliveryEntry, Box<UnreadableClaimedDelivery>>) -> DeliveryClaim {
    match claimed {
        Ok(entry) => DeliveryClaim::Readable(Box::new(entry)),
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

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        queue::{delivery_claim, held},
        rows::{ClaimedDeliveryError, UnreadableClaimedDelivery},
    };
    use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use andromeda_notification::{
        delivery_claim::DeliveryClaim,
        delivery_entry::{DeliveryEntry, DeliveryKey},
        delivery_lease::DeliveryLeaseError,
        notification_event::{NotificationEvent, NotificationEventKey},
    };
    use chrono::Utc;
    use std::sync::Arc;
    use uuid::Uuid;

    fn delivery() -> DeliveryEntry {
        DeliveryEntry::builder()
            .target(ContextTarget::Live)
            .key(DeliveryKey::from(11_i64))
            .subscription_id("urn:ngsi-ld:Subscription:1".parse().expect("a valid subscription URI"))
            .attempt_count(1)
            .next_attempt_at(Utc::now())
            .event(Arc::new(
                NotificationEvent::builder()
                    .key(NotificationEventKey::from(3_i64))
                    .write_id(WriteId::from_uuid(Uuid::nil()))
                    .event_ordinal(WriteOrdinal::at(2))
                    .change(EntityChange::new(EntityLifecycle::Created, Vec::new()))
                    .entity_id("urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"))
                    .created_at(Utc::now())
                    .build(),
            ))
            .build()
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

    /// An unreadable row is still leased to the worker, so it reaches the
    /// worker with the key, branch and reason its acknowledgement records
    /// rather than being dropped from the claim.
    #[test]
    fn an_unreadable_claimed_row_reaches_the_worker_to_be_acknowledged() {
        let unreadable = Box::new(UnreadableClaimedDelivery {
            target: ContextTarget::Live,
            delivery_key: DeliveryKey::from(11_i64),
            reason: ClaimedDeliveryError::EventNotRead {
                event_key: NotificationEventKey::from(3_i64),
            },
        });
        match delivery_claim(Err(unreadable)) {
            DeliveryClaim::Unreadable(claim) => {
                pretty_assertions::assert_eq!(claim.key, DeliveryKey::from(11_i64));
                pretty_assertions::assert_eq!(claim.target, ContextTarget::Live);
                pretty_assertions::assert_eq!(claim.diagnostic, "notification delivery 11 answers event 3, which the claim did not read back");
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
