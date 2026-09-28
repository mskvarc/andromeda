//! Entity change events committed for downstream consumers.

use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity_change::EntityChange,
    notification::{ChangeOrigin, SysAttrsSnapshot},
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// One write's change to one Entity, which clause 5.8.6 notifies once to every
/// subscription it is owed to.
#[derive(Clone, Debug, TypedBuilder)]
pub struct ChangeEvent {
    /// Mutated entity identifier.
    pub entity_id: Uri,
    /// Types declared by the mutated entity.
    #[builder(default = Default::default())]
    pub entity_types: Vec<EntityTypeName>,
    /// What the write did to the Entity and to each Attribute instance it
    /// touched, which the clause 5.2.12 triggers are read from.
    pub change: EntityChange,
    /// Time at which the mutation occurred.
    pub occurred_at: DateTime<Utc>,
    /// Expanded pre-deletion snapshot for an entity deletion.
    #[builder(default = Default::default(), setter(transform = |snapshot: Option<Value>| snapshot.map(Arc::new)))]
    pub entity_snapshot: Option<Arc<Value>>,
    /// Expanded state before the mutation for `showChanges`.
    #[builder(default = Default::default(), setter(transform = |previous: Option<Value>| previous.map(Arc::new)))]
    pub previous_attributes: Option<Arc<Value>>,
    /// System attributes captured before deletion.
    #[builder(default = Default::default())]
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Whether the event originated locally or from a forwarded source.
    pub origin: ChangeOrigin,
    /// Expanded state after the mutation, when already available.
    #[builder(default = Default::default())]
    pub current_payload: Option<Arc<Value>>,
}

#[cfg(test)]
mod tests {
    use crate::change_event::ChangeEvent;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::{ChangeOrigin, NotificationTrigger},
    };
    use chrono::Utc;

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
