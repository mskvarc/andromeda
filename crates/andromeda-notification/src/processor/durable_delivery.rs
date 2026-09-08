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
    processor::NotificationProcessor,
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger, SysAttrsSnapshot};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

impl NotificationDeliveryTransport for NotificationProcessor {
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, Result<DeliveryDisposition, NotificationError>> {
        Box::pin(async move {
            let stored = &entry.event;
            let is_deletion = matches!(stored.trigger, NotificationTrigger::EntityDeleted | NotificationTrigger::AttributeDeleted);
            let snapshot = stored.entity_snapshot.clone(); // clone: the change event owns the durable payload while the leased delivery remains available for acknowledgement.
            // The deleted row is gone, so its system attributes are read back
            // from the snapshot the delete write embedded them into, restoring
            // the typed carrier the in-process path would have supplied.
            let sys_attrs_snapshot = is_deletion.then(|| sys_attrs_from_snapshot(snapshot.as_ref())).flatten();
            let event = ChangeEvent::builder()
                .entity_id(stored.entity_id.clone()) // clone: the change event and the stored event independently own the identifier.
                .entity_types(entity_types_from_snapshot(snapshot.as_ref()))
                .trigger_reason(stored.trigger)
                .changed_attributes(stored.changed_attributes.clone()) // clone: the change event owns its attribute names through delivery.
                .changed_dataset_id(stored.changed_dataset_id.clone()) // clone: the event owns the instance discriminator.
                .occurred_at(stored.created_at)
                .entity_snapshot(is_deletion.then(|| snapshot.clone()).flatten()) // clone: deletion evaluation consumes its own snapshot value.
                .previous_attributes(stored.previous_snapshot.clone()) // clone: showChanges evaluation owns the prior representation.
                .sys_attrs_snapshot(sys_attrs_snapshot)
                .origin(ChangeOrigin::Local)
                .current_payload((!is_deletion).then(|| snapshot.map(Arc::new)).flatten())
                .build();
            let outcome = self.apply_entity_change_for(entry.target.clone(), event, Some(&entry.subscription_id)).await;
            if outcome.delivered > 0 {
                Ok(DeliveryDisposition::Delivered)
            } else if outcome.retryable_failures > 0 {
                Ok(DeliveryDisposition::RetryableFailure {
                    detail: format!("{} endpoint attempt(s) failed transiently", outcome.retryable_failures),
                })
            } else if outcome.terminal_failures > 0 {
                Ok(DeliveryDisposition::TerminalFailure {
                    detail: format!("{} endpoint attempt(s) failed permanently", outcome.terminal_failures),
                })
            } else {
                Ok(DeliveryDisposition::Obsolete)
            }
        })
    }
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
    use crate::processor::durable_delivery::{entity_types_from_snapshot, sys_attrs_from_snapshot};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use serde_json::json;

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
}
