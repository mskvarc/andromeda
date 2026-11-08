//! The entity state a change event is notified with.
//!
//! A change carries its entity in one of three shapes: the state a write left
//! behind, the state a deletion removed, or nothing but the identifier and
//! types. Which one a notification is built from follows the Entity's
//! lifecycle (clause 5.8.6): a creation or modification is notified with the
//! entity as the write left it, and a deletion with what was deleted.

use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::reader::EntityReader;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
use std::sync::Arc;
use tracing::{debug, error};

/// The entity state a notification of `event` is built from, or `None` when
/// there is nothing left to notify about.
pub(super) async fn notified_state(reader: &dyn EntityReader, target: &ContextTarget, event: &ChangeEvent) -> Option<Arc<EntityState>> {
    match event.change.lifecycle {
        EntityLifecycle::Created | EntityLifecycle::Modified => current_state(reader, target, event).await,
        EntityLifecycle::Deleted => removed_state(event),
    }
}

/// The entity as the write left it: the state the event carries, or the
/// stored entity when the event carries none.
async fn current_state(reader: &dyn EntityReader, target: &ContextTarget, event: &ChangeEvent) -> Option<Arc<EntityState>> {
    match event.current_payload.as_ref() {
        Some(state) => Some(Arc::clone(state)),
        None => read_entity(reader, target, event).await,
    }
}

/// The deleted entity: the state the deletion removed, or, without one, its
/// identifier and types alone.
fn removed_state(event: &ChangeEvent) -> Option<Arc<EntityState>> {
    if let Some(removed) = event.entity_snapshot.as_ref() {
        return Some(Arc::clone(removed));
    }
    if let Ok(types) = EntityTypes::try_from_names(event.entity_types.iter().cloned()) {
        return Some(Arc::new(EntityState::new(event.entity_id.clone(), types)));
    }
    debug!(
        "Deletion of {} carries neither a removed state nor a type, skipping notification",
        event.entity_id
    );
    None
}

/// Read the entity the event names from the store, reporting a read failure
/// and answering `None` for an entity that no longer exists.
async fn read_entity(reader: &dyn EntityReader, target: &ContextTarget, event: &ChangeEvent) -> Option<Arc<EntityState>> {
    match reader.get(target, &event.entity_id).await {
        Ok(Some(state)) => Some(state),
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

#[cfg(test)]
mod tests {
    use crate::processor::change_payload::removed_state;
    use andromeda_context_commit::change_event::ChangeEvent;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
    };
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;

    fn deletion_event(snapshot: Option<Arc<EntityState>>, types: Vec<EntityTypeName>) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Vehicle:1".parse().expect("valid URI"))
            .entity_types(types)
            .change(EntityChange::new(EntityLifecycle::Deleted, Vec::new()))
            .occurred_at(Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant"))
            .entity_snapshot(snapshot)
            .origin(ChangeOrigin::Local)
            .build()
    }

    fn vehicle_type() -> EntityTypeName {
        "https://example.com/Vehicle".parse().expect("valid type name")
    }

    /// Clause 5.8.6: a deletion notification is built from the state the
    /// deletion removed, the one allocation the event shares.
    #[test]
    fn an_entity_deletion_is_notified_with_the_removed_state() {
        let removed = Arc::new(EntityState::new(
            "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI"),
            EntityTypes::one(vehicle_type()),
        ));
        let notified = removed_state(&deletion_event(Some(Arc::clone(&removed)), vec![vehicle_type()])).expect("the removed state");
        assert!(Arc::ptr_eq(&notified, &removed));
    }

    #[test]
    fn an_entity_deletion_without_a_snapshot_carries_the_identifier_and_types() {
        let notified = removed_state(&deletion_event(None, vec![vehicle_type()])).expect("the entity's identity");
        pretty_assertions::assert_eq!(notified.id().as_str(), "urn:ngsi-ld:Vehicle:1");
        pretty_assertions::assert_eq!(notified.types().as_slice(), [vehicle_type()].as_slice());
        assert!(notified.attributes().is_empty());
    }

    /// An entity names at least one type (clause 4.5.1), so a deletion that
    /// carries neither its removed state nor a type names no entity to notify.
    #[test]
    fn a_deletion_carrying_neither_a_state_nor_a_type_notifies_nothing() {
        assert!(removed_state(&deletion_event(None, Vec::new())).is_none());
    }
}
