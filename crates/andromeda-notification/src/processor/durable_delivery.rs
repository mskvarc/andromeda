//! Delivering a durably queued notification event through the processor.
//!
//! A queued delivery carries the event as it was persisted, so the change
//! event the processor evaluates is rebuilt from the stored row: the deleted
//! entity's types and system attributes come back from the snapshot the
//! delete write embedded them into, because the row they described is gone.

use crate::{
    delivery_drain::{DeliveryDisposition, NotificationDeliveryTransport},
    delivery_entry::DeliveryEntry,
    notification_event::NotificationEvent,
    processor::NotificationProcessor,
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity_change::EntityLifecycle,
    notification::{ChangeOrigin, SysAttrsSnapshot},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_subscription::error::SubscriptionError;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

impl NotificationDeliveryTransport for NotificationProcessor {
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
        Box::pin(async move {
            let event = queued_change_event(&entry.event);
            if let Err(error) = self.refresh_cached_subscription(&entry.target, &entry.subscription_id).await {
                return unreadable_subscription(&entry.subscription_id, &error);
            }
            let outcome = self.apply_entity_change_for(entry.target.clone(), event, Some(&entry.subscription_id)).await;
            if outcome.delivered > 0 {
                DeliveryDisposition::Delivered
            } else if outcome.retryable_failures > 0 {
                DeliveryDisposition::RetryableFailure {
                    detail: format!("{} endpoint attempt(s) failed transiently", outcome.retryable_failures),
                }
            } else if outcome.terminal_failures > 0 {
                DeliveryDisposition::TerminalFailure {
                    detail: format!("{} endpoint attempt(s) failed permanently", outcome.terminal_failures),
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
fn unreadable_subscription(subscription_id: &Uri, error: &SubscriptionError) -> DeliveryDisposition {
    DeliveryDisposition::RetryableFailure {
        detail: format!("subscription {subscription_id} could not be read back before its notification: {error}"),
    }
}

/// Rebuild the change a durably queued delivery records, as the processor
/// evaluates it.
///
/// The stored lifecycle says whether the Entity outlived the change, so the one
/// stored snapshot is placed accordingly: an Entity the change deleted is
/// carried as what was removed (the shape clause 5.8.6 reduces to `deletedAt`),
/// with its clause 4.8 system attributes read back from the snapshot the delete
/// write embedded them into because the row is gone; any other Entity is
/// carried as the write left it, and its system attributes are read from its
/// row.
fn queued_change_event(stored: &NotificationEvent) -> ChangeEvent {
    let snapshot = stored.entity_snapshot.clone(); // clone: the change event owns the durable payload while the leased delivery remains available for acknowledgement.
    let entity_types = entity_types_from_snapshot(snapshot.as_ref());
    let (removed, current, sys_attrs_snapshot) = match stored.change.lifecycle {
        EntityLifecycle::Deleted => {
            let sys_attrs = sys_attrs_from_snapshot(snapshot.as_ref());
            (snapshot, None, sys_attrs)
        }
        EntityLifecycle::Created | EntityLifecycle::Modified => (None, snapshot.map(Arc::new), None),
    };
    ChangeEvent::builder()
        .entity_id(stored.entity_id.clone()) // clone: the change event and the stored event independently own the identifier.
        .entity_types(entity_types)
        .change(stored.change.clone()) // clone: the change event owns the change it notifies through delivery.
        .occurred_at(stored.created_at)
        .entity_snapshot(removed)
        .previous_attributes(stored.previous_snapshot.clone()) // clone: showChanges evaluation owns the prior representation.
        .sys_attrs_snapshot(sys_attrs_snapshot)
        .origin(ChangeOrigin::Local)
        .current_payload(current)
        .build()
}

/// Reconstruct the clause-4.8 system attributes from a persisted deletion
/// snapshot.
///
/// A durable delete notification cannot re-read `createdAt`/`modifiedAt` from
/// the store — the row is gone — so the delete write embeds them into the
/// stored `entity_snapshot`. Reading them back here restores the typed
/// [`SysAttrsSnapshot`] the in-process path carries, so the same injection runs
/// for both the entity-level and the attribute-level deletion notification.
/// Returns `None` when the snapshot carries no `createdAt` member (a non-deletion
/// snapshot, whose sys attributes are re-read from the live entity instead).
fn sys_attrs_from_snapshot(snapshot: Option<&Value>) -> Option<SysAttrsSnapshot> {
    let object = match snapshot? {
        Value::Array(values) => values.first().and_then(Value::as_object),
        Value::Object(object) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }?;
    let read = |iri: &str| -> Option<DateTime<Utc>> {
        object
            .get(iri)
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(|entry| entry.get("@value"))
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
    };
    let created_at = read(expanded_properties::CREATED_AT.as_str())?;
    Some(SysAttrsSnapshot {
        created_at,
        modified_at: read(expanded_properties::MODIFIED_AT.as_str()).unwrap_or(created_at),
        expires_at: read(expanded_properties::EXPIRES_AT.as_str()),
    })
}

/// Read expanded entity types from a persisted event snapshot.
fn entity_types_from_snapshot(snapshot: Option<&Value>) -> Vec<EntityTypeName> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    let object = match snapshot {
        Value::Array(values) => values.first().and_then(Value::as_object),
        Value::Object(object) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };
    object
        .and_then(|object| object.get("@type"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|value| value.parse::<EntityTypeName>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_drain::DeliveryDisposition,
        notification_event::{NotificationEvent, NotificationEventKey},
        processor::durable_delivery::{entity_types_from_snapshot, queued_change_event, sys_attrs_from_snapshot, unreadable_subscription},
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        expanded::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use andromeda_subscription::error::SubscriptionError;
    use chrono::Utc;
    use serde_json::{Value, json};
    use uuid::Uuid;

    /// The attribute the deliveries below name.
    const NAME: &str = "https://uri.etsi.org/ngsi-ld/default-context/name";

    /// An expanded Entity carrying `name` and `airQualityLevel`, with the
    /// system attributes a delete write embeds.
    fn entity_with_name() -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            expanded_properties::CREATED_AT.as_str(): [{"@value": "2024-01-01T00:00:00Z"}],
            NAME: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Tower"}]}],
            "https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}]
            }]
        }])
    }

    /// The same Entity as a clause 5.6.5 Attribute deletion leaves it.
    fn entity_without_name() -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            "https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}]
            }]
        }])
    }

    /// One queued delivery of a change through `lifecycle` that deleted the
    /// `name` Attribute.
    fn queued(lifecycle: EntityLifecycle, snapshot: Option<Value>) -> NotificationEvent {
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
            .created_at(Utc::now())
            .build()
    }

    /// Clause 5.6.5 removes one Attribute and leaves the Entity: the queued
    /// change is a modification, rebuilt as the Entity the write left behind,
    /// so clause 5.8.6 notifies the deleted Attribute as `urn:ngsi-ld:null`
    /// rather than reducing the Entity to `deletedAt`.
    #[test]
    fn a_queued_attribute_deletion_is_rebuilt_as_the_entity_it_left_behind() {
        let event = queued_change_event(&queued(EntityLifecycle::Modified, Some(entity_without_name())));
        pretty_assertions::assert_eq!(event.entity_id.as_str(), "urn:ngsi-ld:Building:1");
        pretty_assertions::assert_eq!(event.change.lifecycle, EntityLifecycle::Modified);
        pretty_assertions::assert_eq!(event.current_payload.as_deref(), Some(&entity_without_name()));
        assert!(event.entity_snapshot.is_none());
        assert!(
            event.sys_attrs_snapshot.is_none(),
            "a surviving Entity's system attributes are read from its row"
        );
    }

    /// Clause 5.6.6 removes the Entity: the queued change is a deletion,
    /// rebuilt as what was removed with the system attributes its snapshot
    /// embeds, whatever instances it names.
    #[test]
    fn a_queued_entity_deletion_is_rebuilt_as_what_was_removed() {
        let event = queued_change_event(&queued(EntityLifecycle::Deleted, Some(entity_with_name())));
        pretty_assertions::assert_eq!(event.change.lifecycle, EntityLifecycle::Deleted);
        assert!(event.current_payload.is_none());
        pretty_assertions::assert_eq!(event.entity_snapshot.as_deref(), Some(&entity_with_name()));
        let sys_attrs = event.sys_attrs_snapshot.expect("the embedded system attributes");
        pretty_assertions::assert_eq!(sys_attrs.created_at.to_rfc3339(), "2024-01-01T00:00:00+00:00");
        pretty_assertions::assert_eq!(
            event.entity_types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>(),
            vec!["https://uri.etsi.org/ngsi-ld/default-context/Building"]
        );
    }

    /// A delivery with no snapshot leaves the Entity to be read from the
    /// store.
    #[test]
    fn a_queued_modification_without_a_snapshot_leaves_the_entity_to_be_read() {
        let event = queued_change_event(&queued(EntityLifecycle::Modified, None));
        assert!(event.current_payload.is_none());
        assert!(event.entity_snapshot.is_none());
    }

    #[test]
    fn a_deletion_snapshot_gives_back_its_system_attributes() {
        let snapshot = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            expanded_properties::CREATED_AT.as_str(): [{"@value": "2024-01-01T00:00:00Z"}],
            expanded_properties::MODIFIED_AT.as_str(): [{"@value": "2024-01-02T00:00:00Z"}]
        }]);
        let restored = sys_attrs_from_snapshot(Some(&snapshot)).expect("system attributes");
        pretty_assertions::assert_eq!(restored.created_at.to_rfc3339(), "2024-01-01T00:00:00+00:00");
        pretty_assertions::assert_eq!(restored.modified_at.to_rfc3339(), "2024-01-02T00:00:00+00:00");
        assert!(restored.expires_at.is_none());
    }

    #[test]
    fn a_snapshot_without_created_at_carries_no_system_attributes() {
        assert!(sys_attrs_from_snapshot(Some(&json!({"@id": "urn:ngsi-ld:Vehicle:1"}))).is_none());
        assert!(sys_attrs_from_snapshot(None).is_none());
    }

    #[test]
    fn the_types_of_a_wrapped_snapshot_are_read_back() {
        let snapshot = json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.com/Vehicle"]}]);
        let types = entity_types_from_snapshot(Some(&snapshot));
        pretty_assertions::assert_eq!(
            types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/Vehicle"]
        );
        assert_eq!(entity_types_from_snapshot(None), [] as [EntityTypeName; 0]);
    }

    /// A subscription the store could not read back is not a subscription that
    /// is gone: the delivery is retried rather than acknowledged as obsolete,
    /// which would lose the notification it owes.
    #[test]
    fn a_subscription_the_store_cannot_read_back_retries_the_delivery() {
        let subscription_id = "urn:ngsi-ld:Subscription:1".parse().expect("valid subscription URI");
        let disposition = unreadable_subscription(&subscription_id, &SubscriptionError::persistence("the subscription store is unreachable"));
        match disposition {
            DeliveryDisposition::RetryableFailure { detail } => {
                assert!(detail.contains("urn:ngsi-ld:Subscription:1"), "{detail}");
                assert!(detail.contains("the subscription store is unreachable"), "{detail}");
            }
            other @ (DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete | DeliveryDisposition::TerminalFailure { .. }) => {
                panic!("an unreadable subscription must be retried, got {other:?}")
            }
        }
    }
}
