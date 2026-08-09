//! The entity change event the service layer emits and the subscription,
//! outbox and index consumers read (ETSI GS CIM 009 clause 5.3).

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger, SysAttrsSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// An entity mutation that may trigger notifications (ETSI GS CIM 009 clause 5.3.3).
///
/// Construction goes through the [`TypedBuilder`] derive, or [`ChangeEvent::new`]
/// for the common local mutation that carries only its base fields.
#[derive(Clone, Debug, TypedBuilder)]
pub struct ChangeEvent {
    /// Identifier of the mutated entity.
    pub entity_id: Uri,
    /// Types the mutated entity declares.
    #[builder(default)]
    pub entity_types: Vec<EntityTypeName>,
    /// What kind of mutation this was (clause 5.3.3).
    pub trigger_reason: NotificationTrigger,
    /// Names of the attributes the mutation touched.
    #[builder(default)]
    pub changed_attributes: Vec<AttributeName>,
    /// When the mutation happened.
    pub occurred_at: DateTime<Utc>,
    /// For EntityDeleted events, a snapshot of the entity before deletion (expanded form).
    #[builder(default, setter(transform = |snapshot: Option<Value>| snapshot.map(Arc::new)))]
    pub entity_snapshot: Option<Arc<Value>>,
    /// Entity state before the mutation (expanded form), for showChanges support.
    #[builder(default, setter(transform = |previous: Option<Value>| previous.map(Arc::new)))]
    pub previous_attributes: Option<Arc<Value>>,
    /// For EntityDeleted events, a snapshot of (createdAt, modifiedAt, expiresAt) captured before deletion.
    #[builder(default)]
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Whether this event originated locally or from a forwarded context source (clause 5.5.13).
    pub origin: ChangeOrigin,
    /// Post-mutation expanded entity payload, when the emitter has it in
    /// memory (every service write path does). Consumers that would
    /// otherwise re-fetch from the database (e.g. spatial index sync) read
    /// this directly to avoid an extra query per event during batch ingest.
    /// `None` for events emitted by background workers that only know the
    /// entity id (e.g. expiration sweeps); consumers fall back to fetching.
    #[builder(default)]
    pub current_payload: Option<Arc<Value>>,
}

impl ChangeEvent {
    /// Build an event for a mutation that has just happened locally.
    ///
    /// The optional fields default; a caller that needs any of them uses the
    /// [`TypedBuilder`] directly.
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
    use crate::event::change_event::ChangeEvent;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger, SysAttrsSnapshot};
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::json;
    use std::sync::Arc;

    /// The instant every fixture in this module is stamped with.
    fn instant() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant")
    }

    #[test]
    fn new_records_its_trigger_reason_and_marks_the_event_local() {
        let event = ChangeEvent::new(
            "urn:ngsi-ld:Building:1".parse().expect("valid entity id"),
            vec!["Building".parse().expect("valid type name")],
            NotificationTrigger::EntityCreated,
        );
        pretty_assertions::assert_eq!(event.trigger_reason, NotificationTrigger::EntityCreated);
        pretty_assertions::assert_eq!(event.origin, ChangeOrigin::Local);
    }

    /// One builder, no post-construction mutators (R2-123). The snapshot setter
    /// takes a bare value and the previous setter an `Option`; both wrap it in
    /// the shared `Arc` the event stores.
    #[test]
    fn the_builder_wraps_the_optional_payloads_it_is_given() {
        let snapshot = json!({"speed": 10});
        let previous = json!({"speed": 5});
        let current = Arc::new(json!({"speed": 15}));
        let sys_attrs = SysAttrsSnapshot {
            created_at: instant(),
            modified_at: instant(),
            expires_at: Some(instant()),
        };
        let event = ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity id"))
            .entity_types(vec!["Vehicle".parse().expect("valid type name")])
            .trigger_reason(NotificationTrigger::EntityUpdated)
            .changed_attributes(vec!["speed".parse().expect("valid attribute name")])
            .occurred_at(instant())
            .entity_snapshot(Some(snapshot.clone())) // clone: asserted against below
            .previous_attributes(Some(previous.clone())) // clone: asserted against below
            .sys_attrs_snapshot(Some(sys_attrs))
            .origin(ChangeOrigin::Forwarded)
            .current_payload(Some(Arc::clone(&current)))
            .build();

        pretty_assertions::assert_eq!(event.entity_snapshot.as_deref(), Some(&snapshot));
        pretty_assertions::assert_eq!(event.previous_attributes.as_deref(), Some(&previous));
        pretty_assertions::assert_eq!(event.current_payload.as_deref(), Some(&*current));
        pretty_assertions::assert_eq!(event.origin, ChangeOrigin::Forwarded);
        pretty_assertions::assert_eq!(event.sys_attrs_snapshot.expect("sys attrs present").expires_at, Some(instant()));
    }

    #[test]
    fn every_optional_field_defaults_to_unset() {
        let event = ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity id"))
            .trigger_reason(NotificationTrigger::EntityCreated)
            .occurred_at(instant())
            .origin(ChangeOrigin::Local)
            .build();
        assert!(event.entity_types.is_empty() && event.changed_attributes.is_empty());
        assert!(event.entity_snapshot.is_none() && event.previous_attributes.is_none());
        assert!(event.sys_attrs_snapshot.is_none() && event.current_payload.is_none());
    }
}
