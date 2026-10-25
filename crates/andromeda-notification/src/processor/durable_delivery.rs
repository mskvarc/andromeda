//! Delivering a durably queued notification event through the processor.
//!
//! A queued delivery carries the event as it was persisted, so the change
//! event the processor evaluates is rebuilt from the stored row: the deleted
//! entity's types come back from the state it removed and its system
//! attributes from the event, because the row they described is gone.

use crate::{
    delivery_drain::{DeliveryDisposition, NotificationDeliveryTransport},
    delivery_entry::DeliveryEntry,
    notification_event::NotificationEvent,
    processor::{
        NotificationProcessor,
        change_audience::{ChangeAudience, CommitFilters},
        delivery_failure::DeliveryFailure,
    },
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, notification::ChangeOrigin};
use andromeda_subscription::error::SubscriptionError;
use futures::future::BoxFuture;
use std::sync::Arc;

impl NotificationDeliveryTransport for NotificationProcessor {
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
        Box::pin(async move {
            let event = queued_change_event(&entry.event);
            if let Err(error) = self.refresh_cached_subscription(&entry.target, &entry.subscription_id).await {
                return unreadable_subscription(&entry.subscription_id, error);
            }
            let audience = ChangeAudience::Queued {
                subscription: &entry.subscription_id,
                filters: commit_filters(&entry.event),
            };
            let outcome = self.apply_entity_change_for(entry.target.clone(), event, audience).await;
            if outcome.delivered > 0 {
                DeliveryDisposition::Delivered
            } else if outcome.retryable_failures > 0 {
                DeliveryDisposition::RetryableFailure {
                    detail: DeliveryFailure::TransientEndpoints {
                        attempts: outcome.retryable_failures,
                    }
                    .to_string(),
                }
            } else if outcome.terminal_failures > 0 {
                DeliveryDisposition::TerminalFailure {
                    detail: DeliveryFailure::PermanentEndpoints {
                        attempts: outcome.terminal_failures,
                    }
                    .to_string(),
                }
            } else {
                DeliveryDisposition::Obsolete
            }
        })
    }
}

/// The disposition of a delivery whose subscription could not be read back.
///
/// Without the subscription as it now stands the change cannot be matched
/// against it, and reading the failure as a subscription that is gone would
/// acknowledge the delivery as obsolete -- losing a notification clause 5.8.6
/// owes the subscriber. A store that could not answer may answer the next
/// attempt, so the delivery is retried. No notification was attempted, so no
/// subscription bookkeeping (clause 5.2.14.2 `timesSent`, `timesFailed`)
/// records it.
fn unreadable_subscription(subscription_id: &Identifier, error: SubscriptionError) -> DeliveryDisposition {
    DeliveryDisposition::RetryableFailure {
        detail: DeliveryFailure::SubscriptionUnreadable {
            // clone: the failure owns the identifier it names, the entry keeps its own
            subscription_id: subscription_id.clone(),
            source: error,
        }
        .to_string(),
    }
}

/// Whether the commit decided the entity filters of the queued delivery.
///
/// The commit reads the entity filters on the states the change carries: a
/// deletion on the state before it or the state it removed, any other change on
/// the state it left and, when it removed instances, the state before it. A
/// queued event carrying one of those states was filtered on it; one carrying
/// none was admitted without a state to read.
fn commit_filters(stored: &NotificationEvent) -> CommitFilters {
    let filtered = match stored.change.lifecycle {
        EntityLifecycle::Deleted => stored.entity_snapshot.is_some() || stored.previous_snapshot.is_some(),
        EntityLifecycle::Created | EntityLifecycle::Modified => {
            stored.entity_snapshot.is_some() || (stored.previous_snapshot.is_some() && stored.change.deletes_instances())
        }
    };
    if filtered { CommitFilters::Decided } else { CommitFilters::Undecided }
}

/// Rebuild the change a durably queued delivery records, as the processor
/// evaluates it.
///
/// The stored lifecycle says whether the Entity outlived the change, so the one
/// stored state is placed accordingly: an Entity the change deleted is carried
/// as what was removed (the shape clause 5.8.6 reduces to `deletedAt`), with
/// the clause 4.8 system attributes the event recorded because the row is gone;
/// any other Entity is carried as the write left it, and its system attributes
/// are read from its row.
fn queued_change_event(stored: &NotificationEvent) -> ChangeEvent {
    let snapshot = stored.entity_snapshot.as_ref().map(Arc::clone);
    // clone: the change event owns the types it is routed by
    let entity_types = snapshot.as_ref().map(|state| state.types().as_slice().to_vec()).unwrap_or_default();
    let (removed, current, sys_attrs_snapshot) = match stored.change.lifecycle {
        // clone: the change event owns the system attributes it notifies with
        EntityLifecycle::Deleted => (snapshot, None, stored.sys_attrs_snapshot.clone()),
        EntityLifecycle::Created | EntityLifecycle::Modified => (None, snapshot, None),
    };
    ChangeEvent::builder()
        .entity_id(stored.entity_id.clone()) // clone: the change event and the stored event independently own the identifier.
        .entity_types(entity_types)
        .change(stored.change.clone()) // clone: the change event owns the change it notifies through delivery.
        .occurred_at(stored.created_at)
        .entity_snapshot(removed)
        .previous_attributes(stored.previous_snapshot.as_ref().map(Arc::clone))
        .sys_attrs_snapshot(sys_attrs_snapshot)
        .origin(ChangeOrigin::Local)
        .current_payload(current)
        .build()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        delivery_drain::DeliveryDisposition,
        notification_event::{NotificationEvent, NotificationEventKey},
        processor::{
            change_audience::CommitFilters,
            durable_delivery::{commit_filters, queued_change_event, unreadable_subscription},
        },
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::SysAttrsSnapshot,
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_subscription::error::SubscriptionError;
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use uuid::Uuid;

    /// The attribute the deliveries below name.
    const NAME: &str = "https://uri.etsi.org/ngsi-ld/default-context/name";

    /// Decode a stored document into a state.
    fn state(document: &Value) -> Arc<EntityState> {
        Arc::new(EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes"))
    }

    /// An Entity carrying `name` and `airQualityLevel`.
    fn entity_with_name() -> Arc<EntityState> {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            NAME: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Tower"}]}],
            "https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}]
            }]
        }]))
    }

    /// The same Entity as a clause 5.6.5 Attribute deletion leaves it.
    fn entity_without_name() -> Arc<EntityState> {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            "https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}]
            }]
        }]))
    }

    /// The system attributes a deletion recorded.
    fn recorded_instants() -> SysAttrsSnapshot {
        SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("valid instant"),
            modified_at: Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).single().expect("valid instant"),
            expires_at: None,
        }
    }

    /// One queued delivery of a change through `lifecycle` that deleted the
    /// `name` Attribute.
    fn queued(lifecycle: EntityLifecycle, snapshot: Option<Arc<EntityState>>, sys_attrs: Option<SysAttrsSnapshot>) -> NotificationEvent {
        let name = AttributeChange {
            key: AttributeInstanceKey {
                attr: NAME.parse().expect("valid attribute name"),
                dataset_id: None,
            },
            kind: AttributeChangeKind::Deleted,
        };
        NotificationEvent::builder()
            .key(NotificationEventKey::from(1_i64))
            .write_id(WriteId::from_uuid(Uuid::nil()))
            .event_ordinal(WriteOrdinal::at(0))
            .change(EntityChange::new(lifecycle, vec![name]))
            .entity_id("urn:ngsi-ld:Building:1".parse().expect("valid entity URI"))
            .entity_snapshot(snapshot)
            .sys_attrs_snapshot(sys_attrs)
            .created_at(Utc::now())
            .build()
    }

    /// Clause 5.6.5 removes one Attribute and leaves the Entity: the queued
    /// change is a modification, rebuilt as the Entity the write left behind,
    /// so clause 5.8.6 notifies the deleted Attribute as `urn:ngsi-ld:null`
    /// rather than reducing the Entity to `deletedAt`.
    #[test]
    fn a_queued_attribute_deletion_is_rebuilt_as_the_entity_it_left_behind() {
        let left_behind = entity_without_name();
        let event = queued_change_event(&queued(EntityLifecycle::Modified, Some(Arc::clone(&left_behind)), None));
        pretty_assertions::assert_eq!(event.entity_id.as_str(), "urn:ngsi-ld:Building:1");
        pretty_assertions::assert_eq!(event.change.lifecycle, EntityLifecycle::Modified);
        assert!(event.current_payload.as_ref().is_some_and(|current| Arc::ptr_eq(current, &left_behind)));
        assert!(event.entity_snapshot.is_none());
        assert!(
            event.sys_attrs_snapshot.is_none(),
            "a surviving Entity's system attributes are read from its row"
        );
    }

    /// Clause 5.6.6 removes the Entity: the queued change is a deletion,
    /// rebuilt as what was removed with the system attributes the event
    /// recorded, whatever instances it names.
    #[test]
    fn a_queued_entity_deletion_is_rebuilt_as_what_was_removed() {
        let removed = entity_with_name();
        let event = queued_change_event(&queued(EntityLifecycle::Deleted, Some(Arc::clone(&removed)), Some(recorded_instants())));
        pretty_assertions::assert_eq!(event.change.lifecycle, EntityLifecycle::Deleted);
        assert!(event.current_payload.is_none());
        assert!(event.entity_snapshot.as_ref().is_some_and(|snapshot| Arc::ptr_eq(snapshot, &removed)));
        pretty_assertions::assert_eq!(event.sys_attrs_snapshot, Some(recorded_instants()));
        pretty_assertions::assert_eq!(
            event.entity_types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>(),
            vec!["https://uri.etsi.org/ngsi-ld/default-context/Building"]
        );
    }

    /// A delivery with no snapshot leaves the Entity to be read from the
    /// store, and its types unknown.
    #[test]
    fn a_queued_modification_without_a_snapshot_leaves_the_entity_to_be_read() {
        let event = queued_change_event(&queued(EntityLifecycle::Modified, None, None));
        assert!(event.current_payload.is_none());
        assert!(event.entity_snapshot.is_none());
        assert_eq!(event.entity_types, Vec::<EntityTypeName>::new());
    }

    /// A delivery queued with the state the commit filtered on was decided
    /// at commit; one queued without any state was admitted undecided.
    #[test]
    fn a_queued_state_carries_the_commit_filter_decision() {
        assert!(matches!(
            commit_filters(&queued(EntityLifecycle::Modified, Some(entity_without_name()), None)),
            CommitFilters::Decided
        ));
        assert!(matches!(
            commit_filters(&queued(EntityLifecycle::Deleted, Some(entity_with_name()), None)),
            CommitFilters::Decided
        ));
        assert!(matches!(
            commit_filters(&queued(EntityLifecycle::Modified, None, None)),
            CommitFilters::Undecided
        ));
    }

    /// A subscription the store could not read back is not a subscription that
    /// is gone: the delivery is retried rather than acknowledged as obsolete,
    /// which would lose the notification it owes.
    #[test]
    fn a_subscription_the_store_cannot_read_back_retries_the_delivery() {
        let subscription_id = "urn:ngsi-ld:Subscription:1".parse().expect("valid subscription URI");
        let disposition = unreadable_subscription(&subscription_id, SubscriptionError::from(PersistenceFailure::Unavailable));
        match disposition {
            DeliveryDisposition::RetryableFailure { detail } => {
                assert!(detail.contains("urn:ngsi-ld:Subscription:1"), "{detail}");
                assert!(detail.contains("the datastore is temporarily unavailable"), "{detail}");
            }
            other @ (DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete | DeliveryDisposition::TerminalFailure { .. }) => {
                panic!("an unreadable subscription must be retried, got {other:?}")
            }
        }
    }
}
