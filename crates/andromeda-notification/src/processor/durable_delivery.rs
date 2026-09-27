//! Delivering a durably queued notification event through the processor.
//!
//! A queued delivery carries the event as it was persisted, so the change
//! event the processor evaluates is rebuilt from the stored row: the deleted
//! entity's types and system attributes come back from the snapshot the
//! delete write embedded them into, because the row they described is gone.

use crate::{
    delivery_drain::{DeliveryDisposition, NotificationDeliveryTransport},
    delivery_entry::DeliveryEntry,
    error::NotificationError,
    notification_event::NotificationEvent,
    processor::{NotificationProcessor, payload::EntitySurvival},
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::instance_key::attribute_instance_family,
    notification::{ChangeOrigin, NotificationTrigger, SysAttrsSnapshot},
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
            let event = match queued_change_event(&entry.event) {
                Ok(event) => event,
                Err(unevaluable) => return unevaluable,
            };
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
/// # Errors
///
/// Returns the [`DeliveryDisposition::TerminalFailure`] of a delivery whose
/// stored event cannot be evaluated. The stored event is immutable, so every
/// later attempt would meet the same failure: retrying it would only hold its
/// lease, and leaving it unacknowledged would have it claimed again at every
/// lease expiry, forever. No notification was attempted, so no subscription
/// bookkeeping (clause 5.2.14.2 `timesSent`, `timesFailed`) records it.
fn queued_change_event(stored: &NotificationEvent) -> Result<ChangeEvent, DeliveryDisposition> {
    let survival = stored_entity_survival(stored).map_err(|error| DeliveryDisposition::TerminalFailure {
        detail: format!("the queued notification event cannot be evaluated: {error}"),
    })?;
    let snapshot = stored.entity_snapshot.clone(); // clone: the change event owns the durable payload while the leased delivery remains available for acknowledgement.
    // The deleted row is gone, so its system attributes are read back from the
    // snapshot the delete write embedded them into, restoring the typed carrier
    // the in-process path would have supplied. An Entity that survived its
    // change still has a row to read them from.
    let sys_attrs_snapshot = match survival {
        EntitySurvival::Deleted => sys_attrs_from_snapshot(snapshot.as_ref()),
        EntitySurvival::Survived => None,
    };
    Ok(ChangeEvent::builder()
        .entity_id(stored.entity_id.clone()) // clone: the change event and the stored event independently own the identifier.
        .entity_types(entity_types_from_snapshot(snapshot.as_ref()))
        .trigger_reason(stored.trigger)
        .changed_attributes(stored.changed_attributes.clone()) // clone: the change event owns its attribute names through delivery.
        .changed_dataset_id(stored.changed_dataset_id.clone()) // clone: the event owns the instance discriminator.
        .occurred_at(stored.created_at)
        // Which of the two members the stored snapshot fills is what tells the
        // notification path whether the Entity is still there:
        // `entity_snapshot` alone is the shape clause 5.8.6 reduces to
        // `deletedAt`, while `current_payload` is the Entity the change left
        // behind.
        .entity_snapshot(match survival {
            // clone: deletion evaluation consumes its own snapshot value.
            EntitySurvival::Deleted => snapshot.clone(),
            EntitySurvival::Survived => None,
        })
        .previous_attributes(stored.previous_snapshot.clone()) // clone: showChanges evaluation owns the prior representation.
        .sys_attrs_snapshot(sys_attrs_snapshot)
        .origin(ChangeOrigin::Local)
        .current_payload(match survival {
            EntitySurvival::Deleted => None,
            EntitySurvival::Survived => snapshot.map(Arc::new),
        })
        .build())
}

/// Whether the Entity a durably queued delivery names outlived the change it
/// records.
///
/// Every trigger but the two deletions leaves the Entity behind. An
/// `entityDeleted` delivery says outright that it does not. An
/// `attributeDeleted` delivery is the ambiguous one, and the ambiguity is what
/// this resolves: the queue stores one `entity_snapshot` per delivery, and both
/// kinds of deletion fill it.
///
/// Getting this wrong is not a cosmetic difference. Clause 5.8.6 reduces a
/// notification "triggered by the deletion of an Entity" to the `deletedAt`
/// system property alone, so a surviving Entity misread as a deleted one is
/// notified without the very attribute whose deletion triggered it -- the
/// `urn:ngsi-ld:null` that clause 5.5.4 bullet 2 expressly admits "in
/// notifications and the temporal evolution" never reaches the subscriber.
///
/// # Errors
///
/// Returns [`NotificationError::Invariant`] when the stored snapshot carries a
/// `datasetId` that cannot be read: the write that queued the delivery admitted
/// every one, so the instance the delivery names can only be sought in a
/// snapshot the broker itself corrupted.
fn stored_entity_survival(stored: &NotificationEvent) -> Result<EntitySurvival, NotificationError> {
    match stored.trigger {
        NotificationTrigger::EntityDeleted => Ok(EntitySurvival::Deleted),
        NotificationTrigger::AttributeDeleted => attribute_deletion_survival(stored),
        NotificationTrigger::EntityCreated
        | NotificationTrigger::EntityUpdated
        | NotificationTrigger::AttributeCreated
        | NotificationTrigger::AttributeUpdated => Ok(EntitySurvival::Survived),
    }
}

/// Whether an `attributeDeleted` delivery is an ordinary Attribute deletion or
/// the fan-out of an Entity deletion.
///
/// Clause 5.6.5 removes one Attribute and leaves the Entity, so the write
/// stores the Entity it left behind -- which no longer carries the instance the
/// delivery names. Clause 5.6.6 removes the Entity and raises one
/// `attributeDeleted` delivery per Attribute it held, and stores the Entity as
/// it stood *before* the removal, because the notification still has to render
/// its clause 4.8 system attributes once the row is gone; that snapshot does
/// still carry the instance. The instance the delivery names is therefore what
/// tells the two apart, and it is the only thing in the stored row that does.
///
/// A delivery carrying no snapshot at all cannot be placed, and is treated as a
/// surviving Entity so the notification path reads the Entity from the store
/// and drops the notification if it is genuinely gone -- the same reading
/// `crate::processor::payload::entity_survival` gives that shape in process.
fn attribute_deletion_survival(stored: &NotificationEvent) -> Result<EntitySurvival, NotificationError> {
    let Some(snapshot) = stored.entity_snapshot.as_ref() else {
        return Ok(EntitySurvival::Survived);
    };
    if snapshot_carries_deleted_instance(snapshot, stored)? {
        Ok(EntitySurvival::Deleted)
    } else {
        Ok(EntitySurvival::Survived)
    }
}

/// Whether `snapshot` still holds one of the Attribute instances the delivery
/// reports as deleted.
fn snapshot_carries_deleted_instance(snapshot: &Value, stored: &NotificationEvent) -> Result<bool, NotificationError> {
    for attribute in &stored.changed_attributes {
        let family = attribute_instance_family(snapshot, attribute, stored.changed_dataset_id.as_ref())
            .map_err(|error| NotificationError::invariant(format!("queued deletion snapshot: {error}")))?;
        if family.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
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
        processor::{
            durable_delivery::{entity_types_from_snapshot, queued_change_event, stored_entity_survival, sys_attrs_from_snapshot, unreadable_subscription},
            payload::EntitySurvival,
        },
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use andromeda_subscription::error::SubscriptionError;
    use chrono::Utc;
    use serde_json::{Value, json};
    use uuid::Uuid;

    /// The attribute the deletion deliveries below name.
    const NAME: &str = "https://uri.etsi.org/ngsi-ld/default-context/name";

    /// An expanded Entity carrying `name` and `airQualityLevel`.
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

    /// One queued delivery of `trigger` for the `name` Attribute.
    fn queued(trigger: NotificationTrigger, snapshot: Option<Value>) -> NotificationEvent {
        NotificationEvent::builder()
            .key(NotificationEventKey::from(1_i64))
            .write_id(WriteId::from_uuid(Uuid::nil()))
            .event_ordinal(WriteOrdinal::at(0))
            .trigger(trigger)
            .entity_id("urn:ngsi-ld:Building:1".parse().expect("valid entity URI"))
            .changed_attributes(vec![NAME.parse().expect("valid attribute name")])
            .entity_snapshot(snapshot)
            .created_at(Utc::now())
            .build()
    }

    /// Clause 5.6.5 removes one Attribute and leaves the Entity, so the stored
    /// snapshot is the Entity that survived and no longer carries the deleted
    /// instance. Read as a deleted Entity, clause 5.8.6 would reduce the
    /// notification to `deletedAt` and the `urn:ngsi-ld:null` clause 5.5.4
    /// bullet 2 admits "in notifications" would never be delivered.
    #[test]
    fn a_queued_attribute_deletion_that_left_the_entity_behind_is_not_an_entity_deletion() {
        let stored = queued(NotificationTrigger::AttributeDeleted, Some(entity_without_name()));
        pretty_assertions::assert_eq!(stored_entity_survival(&stored).expect("a readable snapshot"), EntitySurvival::Survived);
    }

    /// Clause 5.6.6 deletes the Entity and raises one `attributeDeleted`
    /// delivery per Attribute it held, storing the Entity as it stood before
    /// the removal so the notification can still render its clause 4.8 system
    /// attributes. That snapshot still carries the instance.
    #[test]
    fn a_queued_attribute_deletion_that_went_with_its_entity_is_an_entity_deletion() {
        let stored = queued(NotificationTrigger::AttributeDeleted, Some(entity_with_name()));
        pretty_assertions::assert_eq!(stored_entity_survival(&stored).expect("a readable snapshot"), EntitySurvival::Deleted);
    }

    /// An `entityDeleted` delivery says so outright, whatever it stored.
    #[test]
    fn a_queued_entity_deletion_is_always_an_entity_deletion() {
        let stored = queued(NotificationTrigger::EntityDeleted, Some(entity_with_name()));
        pretty_assertions::assert_eq!(stored_entity_survival(&stored).expect("a readable snapshot"), EntitySurvival::Deleted);
    }

    /// A delivery with no snapshot cannot be placed, so the Entity is read from
    /// the store instead of being assumed gone.
    #[test]
    fn a_queued_attribute_deletion_without_a_snapshot_leaves_the_entity_to_be_read() {
        let stored = queued(NotificationTrigger::AttributeDeleted, None);
        pretty_assertions::assert_eq!(stored_entity_survival(&stored).expect("a readable snapshot"), EntitySurvival::Survived);
    }

    /// Every trigger that is not a deletion leaves the Entity behind.
    #[test]
    fn a_queued_change_that_is_not_a_deletion_leaves_the_entity_behind() {
        for trigger in [
            NotificationTrigger::EntityCreated,
            NotificationTrigger::EntityUpdated,
            NotificationTrigger::AttributeCreated,
            NotificationTrigger::AttributeUpdated,
        ] {
            let stored = queued(trigger, Some(entity_with_name()));
            pretty_assertions::assert_eq!(
                stored_entity_survival(&stored).expect("a readable snapshot"),
                EntitySurvival::Survived,
                "{trigger}"
            );
        }
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

    /// A stored snapshot naming the deleted instance by a `datasetId` that is
    /// not an identifier can never be placed, however often it is retried, so
    /// the delivery is a terminal failure the drain retires rather than an
    /// error that would leave it leased and claimed again at every lease
    /// expiry.
    #[test]
    fn a_queued_event_whose_snapshot_cannot_be_evaluated_is_a_terminal_failure() {
        let unreadable_dataset = json!([{
            "@id": "urn:ngsi-ld:Building:1",
            NAME: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                expanded_properties::DATASET_ID.as_str(): [{"@id": "not a dataset identifier"}],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Tower"}]
            }]
        }]);
        let stored = queued(NotificationTrigger::AttributeDeleted, Some(unreadable_dataset));
        match queued_change_event(&stored) {
            Err(DeliveryDisposition::TerminalFailure { detail }) => assert!(detail.contains("not a dataset identifier"), "{detail}"),
            Err(other) => panic!("an unevaluable event must be a terminal failure, got {other:?}"),
            Ok(_) => panic!("an unevaluable event must not be rebuilt"),
        }
    }

    /// An event that can be evaluated is rebuilt as the change it records.
    #[test]
    fn a_queued_event_that_can_be_evaluated_is_rebuilt_as_its_change() {
        let stored = queued(NotificationTrigger::AttributeDeleted, Some(entity_without_name()));
        let event = queued_change_event(&stored).expect("an evaluable event");
        pretty_assertions::assert_eq!(event.entity_id.as_str(), "urn:ngsi-ld:Building:1");
        pretty_assertions::assert_eq!(event.trigger_reason, NotificationTrigger::AttributeDeleted);
        assert!(event.current_payload.is_some(), "the Entity survived the Attribute deletion");
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
