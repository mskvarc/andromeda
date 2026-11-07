//! Entity change events committed for downstream consumers.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_change::EntityChange,
    notification::{ChangeOrigin, SysAttrsSnapshot},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// One write's change to one Entity, which clause 5.8.6 notifies once to every
/// subscription it is owed to.
#[derive(Clone, Debug, TypedBuilder)]
pub struct ChangeEvent {
    /// Mutated entity identifier.
    pub entity_id: Identifier,
    /// Types declared by the mutated entity.
    #[builder(default = Default::default())]
    pub entity_types: Vec<EntityTypeName>,
    /// What the write did to the Entity and to each Attribute instance it
    /// touched, which the clause 5.2.12 triggers are read from.
    pub change: EntityChange,
    /// Time at which the mutation occurred.
    pub occurred_at: DateTime<Utc>,
    /// The state an entity deletion removed.
    #[builder(default = Default::default())]
    pub entity_snapshot: Option<Arc<EntityState>>,
    /// The state before the mutation, for `showChanges`.
    #[builder(default = Default::default())]
    pub previous_attributes: Option<Arc<EntityState>>,
    /// The Entity's clause 4.8 system attributes as the change left it, or as
    /// a deletion removed it: what a notification of the change is stamped
    /// with (clause 5.8.6), whatever later writes did to the row.
    #[builder(default = Default::default())]
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Whether the event originated locally or from a forwarded source.
    pub origin: ChangeOrigin,
    /// The state after the mutation, when already available.
    #[builder(default = Default::default())]
    pub current_payload: Option<Arc<EntityState>>,
}

#[cfg(test)]
mod tests {
    use crate::change_event::ChangeEvent;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::{ChangeOrigin, NotificationTrigger},
    };
    use chrono::Utc;
    use std::sync::Arc;

    /// A deletion hands both fields the one removed state it read, so the
    /// event carries one allocation however many readers it has.
    #[test]
    fn a_change_event_shares_the_snapshot_it_is_given() {
        let removed = Arc::new(EntityState::new(
            "urn:ngsi-ld:Building:1".parse().expect("valid entity id"),
            EntityTypes::one("https://example.org/Building".parse().expect("valid type name")),
        ));
        let event = ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Building:1".parse().expect("valid entity id"))
            .change(EntityChange::new(EntityLifecycle::Deleted, Vec::new()))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .entity_snapshot(Some(Arc::clone(&removed)))
            .previous_attributes(Some(Arc::clone(&removed)))
            .build();
        let (Some(snapshot), Some(previous)) = (&event.entity_snapshot, &event.previous_attributes) else {
            panic!("both snapshots are carried");
        };
        assert!(Arc::ptr_eq(snapshot, previous));
        assert!(Arc::ptr_eq(snapshot, &removed));
    }

    #[test]
    fn a_change_event_raises_the_triggers_of_its_change() {
        let event = ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Building:1".parse().expect("valid entity id"))
            .entity_types(vec!["Building".parse().expect("valid type name")])
            .change(EntityChange::new(EntityLifecycle::Created, Vec::new()))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build();
        pretty_assertions::assert_eq!(event.change.triggers(), vec![NotificationTrigger::EntityCreated]);
        pretty_assertions::assert_eq!(event.origin, ChangeOrigin::Local);
    }
}
