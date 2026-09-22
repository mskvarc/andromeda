//! The expanded entity document a change event is notified with.
//!
//! A change carries its entity in one of three shapes: the current payload of
//! a write, a snapshot taken before a deletion, or nothing but the identifier
//! and types. Which one a notification is built from follows the trigger
//! (clause 5.8.6): a live change reads the entity as it is now, an attribute
//! deletion reads what remains, and an entity deletion reads what was
//! deleted, stamped with `deletedAt`.

use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::reader::EntityReader;
use andromeda_ngsi_ld_lexicon::rfc3339::sub_second;
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{debug, error};

/// The expanded entity a notification of `event` is built from, or `None`
/// when there is nothing left to notify about.
pub(super) async fn entity_data_for(reader: &dyn EntityReader, target: &ContextTarget, event: &ChangeEvent) -> Option<Value> {
    match event.trigger_reason {
        NotificationTrigger::EntityCreated
        | NotificationTrigger::EntityUpdated
        | NotificationTrigger::AttributeCreated
        | NotificationTrigger::AttributeUpdated => current_entity(reader, target, event).await,
        NotificationTrigger::AttributeDeleted => attribute_deletion_entity(reader, target, event).await,
        NotificationTrigger::EntityDeleted => Some(entity_deletion_entity(event)),
    }
}

/// The entity as the write left it: the payload the event carries, or the
/// stored entity when the event carries none.
async fn current_entity(reader: &dyn EntityReader, target: &ContextTarget, event: &ChangeEvent) -> Option<Value> {
    match event.current_payload.as_deref() {
        Some(payload) => Some(payload.clone()), // clone: the notification owns its document while the event stays shared
        None => read_entity(reader, target, event).await,
    }
}

/// What the notification carries for an attribute deletion: the entity as the
/// write left it, the deleted entity stamped with `deletedAt` when the
/// attribute went with the whole entity, or the stored entity when the event
/// carries neither.
async fn attribute_deletion_entity(reader: &dyn EntityReader, target: &ContextTarget, event: &ChangeEvent) -> Option<Value> {
    if let Some(payload) = event.current_payload.as_deref() {
        return Some(payload.clone()); // clone: the notification owns its document while the event stays shared
    }
    // Clause 5.8.6: "If the notification was triggered by the deletion of an
    // Entity ... only the deletedAt system property shall be provided." Deleting
    // an entity raises one `AttributeDeleted` event per attribute, each carrying
    // the pre-deletion snapshot and no post-mutation payload -- so a snapshot
    // without a payload describes what is gone, not what remains, and is owed
    // the same `deletedAt` the `EntityDeleted` event is.
    if event.entity_snapshot.is_some() {
        return Some(entity_deletion_entity(event));
    }
    read_entity(reader, target, event).await
}

/// The deleted entity: its snapshot stamped with `deletedAt`, or, without a
/// snapshot, its identifier and types alone.
fn entity_deletion_entity(event: &ChangeEvent) -> Value {
    let mut data = match event.entity_snapshot.as_deref() {
        Some(snapshot) => entity_node(snapshot),
        None => serde_json::json!({
            "@id": event.entity_id,
            "@type": event.entity_types,
        }),
    };
    stamp_deleted_at(&mut data, event.occurred_at);
    Value::Array(vec![data])
}

/// Read the entity the event names from the store, reporting a read failure
/// and answering `None` for an entity that no longer exists.
async fn read_entity(reader: &dyn EntityReader, target: &ContextTarget, event: &ChangeEvent) -> Option<Value> {
    match reader.get(target, &event.entity_id).await {
        Ok(Some(data)) => Some(data),
        Ok(None) => {
            debug!("Entity {} no longer exists, skipping notification", event.entity_id);
            None
        }
        Err(e) => {
            error!("Failed to fetch entity {} for notification: {}", event.entity_id, e);
            None
        }
    }
}

/// The node of an expanded entity payload, which is stored either bare or
/// wrapped in a one-element array.
fn entity_node(snapshot: &Value) -> Value {
    match snapshot {
        Value::Array(values) => match values.first() {
            Some(value) => value.clone(), // clone: the node is lifted out of the shared snapshot
            None => snapshot.clone(),     // clone: same
        },
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other.clone(), // clone: same
    }
}

/// Stamp an expanded entity node with the clause 4.8 `deletedAt` timestamp.
fn stamp_deleted_at(node: &mut Value, occurred_at: DateTime<Utc>) {
    if let Some(obj) = node.as_object_mut() {
        obj.insert(
            expanded_properties::DELETED_AT.as_str().to_string(),
            serde_json::json!([{
                "@type": expanded_classes::DATE_TIME.as_str(),
                "@value": sub_second(occurred_at)
            }]),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::processor::change_payload::{entity_deletion_entity, entity_node, stamp_deleted_at};
    use andromeda_context_commit::change_event::ChangeEvent;
    use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};

    fn deletion_event(snapshot: Option<Value>) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Vehicle:1".parse().expect("valid URI"))
            .entity_types(vec!["https://example.com/Vehicle".parse().expect("valid type name")])
            .trigger_reason(NotificationTrigger::EntityDeleted)
            .changed_attributes(Vec::new())
            .occurred_at(Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant"))
            .entity_snapshot(snapshot)
            .origin(ChangeOrigin::Local)
            .build()
    }

    #[test]
    fn a_wrapped_or_bare_snapshot_yields_its_node() {
        let node = json!({"@id": "urn:ngsi-ld:Vehicle:1"});
        pretty_assertions::assert_eq!(entity_node(&json!([node.clone()])), node);
        pretty_assertions::assert_eq!(entity_node(&node), node);
    }

    /// Clause 5.8.6: a deletion notification carries the deleted entity
    /// stamped with `deletedAt`, wrapped the way the store wraps a payload.
    #[test]
    fn an_entity_deletion_stamps_the_snapshot_with_deleted_at() {
        let data = entity_deletion_entity(&deletion_event(Some(
            json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.com/Vehicle"]}]),
        )));
        let node = &data[0];
        pretty_assertions::assert_eq!(node["@id"], json!("urn:ngsi-ld:Vehicle:1"));
        pretty_assertions::assert_eq!(node[expanded_properties::DELETED_AT.as_str()][0]["@value"], json!("2024-06-15T12:30:00Z"));
    }

    #[test]
    fn an_entity_deletion_without_a_snapshot_carries_the_identifier_and_types() {
        let data = entity_deletion_entity(&deletion_event(None));
        let node = &data[0];
        pretty_assertions::assert_eq!(node["@id"], json!("urn:ngsi-ld:Vehicle:1"));
        pretty_assertions::assert_eq!(node["@type"], json!(["https://example.com/Vehicle"]));
        assert!(node.get(expanded_properties::DELETED_AT.as_str()).is_some());
    }

    #[test]
    fn a_non_object_node_is_not_stamped() {
        let mut node = json!("not an entity");
        stamp_deleted_at(&mut node, Utc::now());
        pretty_assertions::assert_eq!(node, json!("not an entity"));
    }
}
