//! Change events and the notification payloads they produce (ETSI GS CIM 009 clause 5.3).

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, constant_string, entity_type_name::EntityTypeName, uri::Uri};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use std::sync::Arc;
use strum::{Display, EnumIter, EnumString};
use typed_builder::TypedBuilder;

constant_string!(NotificationType, "Notification");
constant_string!(CsourceNotificationType, "CsourceNotification");

/// Trigger conditions that cause a notification to be sent (ETSI GS CIM 009 clause 5.3.3).
///
/// One vocabulary serves both sides of clause 5.3.3: a subscription lists the
/// triggers it listens for (`Subscription::notification_trigger`), and a
/// [`ChangeEvent`] carries the one that fired. [`EnumIter`] yields the default
/// set — every trigger — without a hand-written token copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, EnumIter)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum NotificationTrigger {
    /// An entity matching the subscription was created.
    EntityCreated,
    /// An entity matching the subscription was updated.
    EntityUpdated,
    /// An entity matching the subscription was deleted.
    EntityDeleted,
    /// A watched attribute was created on a matching entity.
    AttributeCreated,
    /// A watched attribute was updated on a matching entity.
    AttributeUpdated,
    /// A watched attribute was deleted from a matching entity.
    AttributeDeleted,
}

/// Origin of an entity change event (ETSI GS CIM 009 v1.9.1, clause 5.5.13).
///
/// Distinguishes whether an entity mutation originated from the local broker
/// or was forwarded from a registered context source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOrigin {
    /// The entity is stored and managed by this broker instance.
    Local,
    /// The entity was forwarded from a registered context source.
    Forwarded,
}

/// System attribute timestamps captured before entity deletion.
#[derive(Clone, Debug)]
pub struct SysAttrsSnapshot {
    /// Creation timestamp the entity carried.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp the entity carried.
    pub modified_at: DateTime<Utc>,
    /// Expiration timestamp the entity carried, if any.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Trigger reason for entity notifications (ETSI GS CIM 009 clause 5.3.3).
///
/// Represents an entity mutation that may trigger notifications.
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
    #[builder(default)]
    pub entity_snapshot: Option<Arc<Value>>,
    /// Entity state before the mutation (expanded form), for showChanges support.
    #[builder(default)]
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
    pub fn new(entity_id: Uri, entity_types: Vec<EntityTypeName>, trigger_reason: NotificationTrigger) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id(entity_id)
            .entity_types(entity_types)
            .trigger_reason(trigger_reason)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build()
    }

    /// Record which attributes the mutation touched.
    pub fn with_changed_attributes(mut self, attrs: Vec<AttributeName>) -> ChangeEvent {
        self.changed_attributes = attrs;
        self
    }

    /// Attach the pre-deletion entity snapshot.
    pub fn with_snapshot(mut self, snapshot: Value) -> ChangeEvent {
        self.entity_snapshot = Some(Arc::new(snapshot));
        self
    }

    /// Attach the pre-mutation entity state, for `showChanges`.
    pub fn with_previous(mut self, previous: Value) -> ChangeEvent {
        self.previous_attributes = Some(Arc::new(previous));
        self
    }

    /// Attach the pre-mutation entity state when the caller has it.
    pub fn with_optional_previous(mut self, previous: Option<Value>) -> ChangeEvent {
        self.previous_attributes = previous.map(Arc::new);
        self
    }

    /// Attach the timestamps captured before a deletion.
    pub const fn with_sys_attrs(mut self, sys_attrs: Option<SysAttrsSnapshot>) -> ChangeEvent {
        self.sys_attrs_snapshot = sys_attrs;
        self
    }

    /// Attach the post-mutation expanded payload so payload-aware consumers
    /// (notably the spatial sync worker) skip the per-event entity fetch
    /// during high-throughput batch ingest.
    pub fn with_current_payload(mut self, payload: Arc<Value>) -> ChangeEvent {
        self.current_payload = Some(payload);
        self
    }
}

/// NGSI-LD Notification payload (ETSI GS CIM 009 clause 5.3.1).
///
/// Represents a notification sent to a subscriber when matching entity
/// changes occur. Contains the subscription reference, timestamp, and
/// the entity data that triggered the notification.
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// Generated notification identifier.
    pub id: Uri,
    /// Always `Notification`.
    #[serde(rename = "type")]
    #[builder(default)]
    pub type_: NotificationType,
    /// Subscription this notification answers.
    pub subscription_id: Uri,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The entity representations that triggered it.
    #[builder(default)]
    pub data: Vec<Value>,
}

/// Trigger reason for CSR notifications (ETSI GS CIM 009 clause 5.3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum CsrTriggerReason {
    /// The registration started matching the subscription.
    NewlyMatching,
    /// A matching registration changed.
    Updated,
    /// The registration stopped matching the subscription.
    NoLongerMatching,
}

/// Represents a CSR mutation that may trigger CSR subscription notifications.
#[derive(Clone, Debug)]
pub struct CsrChangeEvent {
    /// Identifier of the mutated registration.
    pub csr_id: Uri,
    /// What kind of mutation this was.
    pub trigger_reason: CsrTriggerReason,
    /// For delete events, a snapshot of the CSR before deletion.
    pub csr_snapshot: Option<Value>,
}

/// NGSI-LD CsourceNotification payload (ETSI GS CIM 009 clause 5.3.2).
///
/// Represents a notification sent to a subscriber when context source
/// registration changes occur. Contains the subscription reference,
/// timestamp, CSR data, and the trigger reason.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct CsourceNotification {
    /// Generated notification identifier.
    pub id: Uri,
    /// Always `CsourceNotification`.
    #[serde(rename = "type")]
    #[builder(default)]
    pub type_: CsourceNotificationType,
    /// Subscription this notification answers.
    pub subscription_id: Uri,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The registration representations that triggered it.
    #[builder(default)]
    pub data: Vec<Value>,
    /// Why the notification fired.
    #[builder(default)]
    pub trigger_reason: Option<CsrTriggerReason>,
}

/// Signals subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    /// The subscription with this id was created or replaced.
    Upserted(Uri),
    /// The subscription with this id was deleted.
    Deleted(Uri),
}

/// Signals CSR subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    /// The CSR subscription with this id was created or replaced.
    Upserted(Uri),
    /// The CSR subscription with this id was deleted.
    Deleted(Uri),
}

#[cfg(test)]
mod tests {
    use crate::notification::{ChangeEvent, ChangeOrigin, CsourceNotification, CsrTriggerReason, Notification, NotificationTrigger, SysAttrsSnapshot};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::{str::FromStr, sync::Arc};
    use strum::IntoEnumIterator;

    /// The instant every fixture in this module is stamped with.
    fn instant() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant")
    }

    /// A subscription identifier the notification fixtures answer for.
    fn subscription_id() -> Uri {
        "urn:ngsi-ld:Subscription:1".parse().expect("valid subscription id")
    }

    #[test]
    fn a_notification_carries_its_fixed_type_token() {
        let notification = Notification::builder()
            .id("urn:ngsi-ld:Notification:1".parse::<Uri>().expect("valid notification id"))
            .subscription_id(subscription_id())
            .notified_at(instant())
            .build();
        let rendered = serde_json::to_value(&notification).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("Notification"));
        pretty_assertions::assert_eq!(rendered["data"], json!([]));
    }

    #[test]
    fn a_csource_notification_carries_its_own_type_token() {
        let notification = CsourceNotification::builder()
            .id("urn:ngsi-ld:CsourceNotification:1".parse::<Uri>().expect("valid notification id"))
            .subscription_id(subscription_id())
            .notified_at(instant())
            .trigger_reason(Some(CsrTriggerReason::NewlyMatching))
            .build();
        let rendered = serde_json::to_value(&notification).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("CsourceNotification"));
        pretty_assertions::assert_eq!(rendered["triggerReason"], json!("newlyMatching"));
    }

    #[test]
    fn a_notification_rejects_any_other_type_token() {
        let error = serde_json::from_value::<Notification>(json!({
            "id": "urn:ngsi-ld:Notification:1",
            "type": "CsourceNotification",
            "subscriptionId": "urn:ngsi-ld:Subscription:1",
            "notifiedAt": "2024-06-15T12:30:00Z",
            "data": []
        }))
        .expect_err("a foreign discriminator is rejected");
        assert!(error.to_string().contains("invalid value"), "{error}");
    }

    #[test]
    fn a_change_event_records_its_trigger_reason() {
        // `NotificationTrigger` reaches the wire through `Display`/`FromStr` rather than
        // serde: it is a database label and a `notificationTrigger` token, never a
        // serialised member of `ChangeEvent`, which is an in-process value.
        for reason in [
            NotificationTrigger::EntityCreated,
            NotificationTrigger::EntityUpdated,
            NotificationTrigger::EntityDeleted,
            NotificationTrigger::AttributeCreated,
            NotificationTrigger::AttributeUpdated,
            NotificationTrigger::AttributeDeleted,
        ] {
            let event = ChangeEvent::new(
                "urn:ngsi-ld:Building:1".parse().expect("valid entity id"),
                vec!["Building".parse().expect("valid type name")],
                reason,
            );
            pretty_assertions::assert_eq!(event.trigger_reason, reason);
            pretty_assertions::assert_eq!(NotificationTrigger::from_str(&reason.to_string()).expect("round trip"), reason);
            // The default trigger set is every trigger (clause 5.3.3), derived
            // from the enum rather than a hand-written token list.
            assert!(NotificationTrigger::iter().any(|trigger| trigger == reason));
        }
    }

    #[test]
    fn a_change_event_builder_round_trips_its_fields() {
        let snapshot = Arc::new(json!({"speed": 10}));
        let previous = Arc::new(json!({"speed": 5}));
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
            .entity_snapshot(Some(Arc::clone(&snapshot)))
            .previous_attributes(Some(Arc::clone(&previous)))
            .sys_attrs_snapshot(Some(sys_attrs))
            .origin(ChangeOrigin::Forwarded)
            .current_payload(Some(Arc::clone(&current)))
            .build();

        pretty_assertions::assert_eq!(event.entity_id.as_ref(), "urn:ngsi-ld:Vehicle:1");
        pretty_assertions::assert_eq!(event.entity_types.len(), 1);
        pretty_assertions::assert_eq!(event.trigger_reason, NotificationTrigger::EntityUpdated);
        pretty_assertions::assert_eq!(event.changed_attributes.len(), 1);
        pretty_assertions::assert_eq!(event.occurred_at, instant());
        pretty_assertions::assert_eq!(event.entity_snapshot.as_deref(), Some(&*snapshot));
        pretty_assertions::assert_eq!(event.previous_attributes.as_deref(), Some(&*previous));
        pretty_assertions::assert_eq!(event.sys_attrs_snapshot.expect("sys attrs present").expires_at, Some(instant()));
        pretty_assertions::assert_eq!(event.origin, ChangeOrigin::Forwarded);
        pretty_assertions::assert_eq!(event.current_payload.as_deref(), Some(&*current));
    }

    #[test]
    fn a_change_event_builder_leaves_every_optional_field_unset() {
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
