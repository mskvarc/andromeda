//! Entity change events committed for downstream consumers.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger, SysAttrsSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// An entity mutation that may trigger notifications under NGSI-LD clause
/// 5.2.12.
#[derive(Clone, Debug, TypedBuilder)]
pub struct ChangeEvent {
    /// Mutated entity identifier.
    pub entity_id: Uri,
    /// Types declared by the mutated entity.
    #[builder(default = Default::default())]
    pub entity_types: Vec<EntityTypeName>,
    /// Mutation trigger from clause 5.2.12.
    pub trigger_reason: NotificationTrigger,
    /// Attribute names touched by the mutation.
    #[builder(default = Default::default())]
    pub changed_attributes: Vec<AttributeName>,
    /// Dataset discriminator of the one changed Attribute instance, or `None`
    /// for the default instance and for entity-level events.
    #[builder(default = Default::default())]
    pub changed_dataset_id: Option<Uri>,
    /// Time at which the mutation occurred.
    pub occurred_at: DateTime<Utc>,
    /// Expanded pre-deletion snapshot for entity-deleted events.
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

impl ChangeEvent {
    /// Build a local event whose optional payload fields are absent.
    #[must_use]
    pub fn new(entity_id: Uri, entity_types: Vec<EntityTypeName>, trigger_reason: NotificationTrigger) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id(entity_id)
            .entity_types(entity_types)
            .trigger_reason(trigger_reason)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use crate::change_event::ChangeEvent;
    use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};

    #[test]
    fn the_convenience_constructor_marks_a_change_local() {
        let event = ChangeEvent::new(
            "urn:ngsi-ld:Building:1".parse().expect("valid entity id"),
            vec!["Building".parse().expect("valid type name")],
            NotificationTrigger::EntityCreated,
        );
        pretty_assertions::assert_eq!(event.trigger_reason, NotificationTrigger::EntityCreated);
        pretty_assertions::assert_eq!(event.origin, ChangeOrigin::Local);
    }
}
