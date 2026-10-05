//! Change events and the notification payloads they produce (ETSI GS CIM 009 clause 5.3).

use crate::{csource_change_revision::CsourceChangeRevision, registration::ContextSourceRegistration};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{constant_string, identifier::Identifier};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, value::RawValue};
use strum::{Display, EnumCount, EnumIter, EnumString};
use typed_builder::TypedBuilder;

constant_string!(NotificationType, "Notification");
constant_string!(CsourceNotificationType, "ContextSourceNotification");

/// Origin of an entity change event (ETSI GS CIM 009 v1.9.1, clause 5.5.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ChangeOrigin {
    /// The entity is stored and managed by this broker instance.
    Local,
    /// The entity was forwarded from a registered context source.
    Forwarded,
}

/// Trigger conditions that cause a notification to be sent (ETSI GS CIM 009
/// v1.9.1, clause 5.2.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, EnumIter, EnumCount)]
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

/// Clause-5.2.12 trigger set used when a Subscription omits
/// `notificationTrigger`.
pub const DEFAULT_NOTIFICATION_TRIGGERS: [NotificationTrigger; 2] = [NotificationTrigger::AttributeCreated, NotificationTrigger::AttributeUpdated];

/// Compile the Subscription trigger vocabulary into precise change triggers.
///
/// Clause 5.2.12 defines `entityUpdated` as equivalent to the combination of
/// `attributeCreated`, `attributeUpdated`, and `attributeDeleted`. This helper
/// performs that expansion on the Subscription side; mutation producers remain
/// responsible for classifying the precise attribute change that occurred.
#[must_use]
pub fn effective_notification_triggers(requested: Option<&[NotificationTrigger]>) -> Vec<NotificationTrigger> {
    let requested = requested.unwrap_or(&DEFAULT_NOTIFICATION_TRIGGERS);
    let mut effective = Vec::with_capacity(requested.len().saturating_add(2));
    for trigger in requested {
        match trigger {
            NotificationTrigger::EntityUpdated => {
                push_unique(&mut effective, NotificationTrigger::AttributeCreated);
                push_unique(&mut effective, NotificationTrigger::AttributeUpdated);
                push_unique(&mut effective, NotificationTrigger::AttributeDeleted);
            }
            NotificationTrigger::EntityCreated
            | NotificationTrigger::EntityDeleted
            | NotificationTrigger::AttributeCreated
            | NotificationTrigger::AttributeUpdated
            | NotificationTrigger::AttributeDeleted => push_unique(&mut effective, *trigger),
        }
    }
    effective
}

/// Append a trigger only when the list does not already carry it.
fn push_unique(triggers: &mut Vec<NotificationTrigger>, trigger: NotificationTrigger) {
    if !triggers.contains(&trigger) {
        triggers.push(trigger);
    }
}

/// System attribute timestamps captured before entity deletion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SysAttrsSnapshot {
    /// Creation timestamp the entity carried.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp the entity carried.
    pub modified_at: DateTime<Utc>,
    /// Expiration timestamp the entity carried, if any.
    pub expires_at: Option<DateTime<Utc>>,
}

/// NGSI-LD Notification sent to a subscriber (ETSI GS CIM 009 clause 5.3.1).
///
/// Written once, to the bytes the endpoint receives: the entities in `data`
/// are already written in the shape the endpoint's media type asks for, and
/// `@context` is present exactly when clause 6.3.8 puts the context in the
/// body.
#[derive(Debug, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Notification<'a> {
    /// The `@context` the body carries, when clause 6.3.8 places it there.
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    pub context: Option<&'a JsonLiteral>,
    /// Generated notification identifier.
    pub id: &'a Identifier,
    /// Always `Notification`.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: NotificationType,
    /// Subscription this notification answers.
    pub subscription_id: &'a Identifier,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The entity representations that triggered it.
    pub data: NotificationData,
}

/// The `data` member of a Notification (Table 5.3.1-1): "NGSI-LD Entity[] or
/// `FeatureCollection`", already written.
///
/// Which one is decided by the subscription's `notification.endpoint.accept`:
/// "If the notification has been triggered from a Subscription that has the
/// notification.endpoint.accept field set to application/geo+json then data is
/// returned as a `FeatureCollection`", and otherwise "Entity[] is returned".
/// The writer that knows the entities makes that choice; the text is carried
/// into the notification as written.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct NotificationData(Box<RawValue>);

impl NotificationData {
    /// The `data` member `written` holds.
    #[must_use]
    pub const fn written(written: Box<RawValue>) -> NotificationData {
        NotificationData(written)
    }

    /// The written member.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.get()
    }
}

/// A Notification as a context source sends it to this broker (clause 5.3.1).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedNotification {
    /// The sender's notification identifier.
    pub id: Identifier,
    /// Always `Notification`.
    #[serde(rename = "type")]
    pub type_: NotificationType,
    /// The subscription the notification answers, as the sender knows it.
    pub subscription_id: Identifier,
    /// When the sender generated the notification.
    pub notified_at: DateTime<Utc>,
    /// The entity representations it carries.
    pub data: ReceivedNotificationData,
}

/// The `data` member of a received Notification (Table 5.3.1-1): "NGSI-LD
/// Entity[] or `FeatureCollection`".
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ReceivedNotificationData {
    /// The notified Entities, each in the format the sender wrote it in.
    Entities(Vec<Value>),
    /// The notified Entities as one clause 5.2.30 `GeoJSON`
    /// `FeatureCollection`.
    FeatureCollection(Map<String, Value>),
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
    pub csr_id: Identifier,
    /// What kind of mutation this was.
    pub trigger_reason: CsrTriggerReason,
    /// For update and delete events, the registration as it stood before.
    pub csr_snapshot: Option<Box<ContextSourceRegistration>>,
    /// The revision the mutation drew, which orders it against every write of
    /// a context source subscription.
    pub revision: CsourceChangeRevision,
}

/// NGSI-LD `CsourceNotification` sent to a subscriber (ETSI GS CIM 009 clause
/// 5.3.2).
///
/// Represents a notification sent to a subscriber when context source
/// registration changes occur. Contains the subscription reference,
/// timestamp, CSR data, and the trigger reason.
#[derive(Debug, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct CsourceNotification<'a> {
    /// The `@context` the body carries, when clause 6.3.8 places it there.
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    pub context: Option<&'a JsonLiteral>,
    /// Generated notification identifier.
    pub id: &'a Identifier,
    /// Always `ContextSourceNotification`.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: CsourceNotificationType,
    /// Subscription this notification answers.
    pub subscription_id: &'a Identifier,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The registrations that triggered it, as the subscriber is notified of
    /// them.
    pub data: &'a [ContextSourceRegistration],
    /// Why the notification fired (Table 5.3.2-1, mandatory).
    pub trigger_reason: CsrTriggerReason,
}

/// A `CsourceNotification` as a context source sends it to this broker
/// (clause 5.3.2).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedCsourceNotification {
    /// The sender's notification identifier.
    pub id: Identifier,
    /// Always `ContextSourceNotification`.
    #[serde(rename = "type")]
    pub type_: CsourceNotificationType,
    /// The subscription the notification answers, as the sender knows it.
    pub subscription_id: Identifier,
    /// When the sender generated the notification.
    pub notified_at: DateTime<Utc>,
    /// The registration representations it carries.
    #[serde(default)]
    pub data: Vec<Value>,
    /// Why the notification fired.
    #[serde(default)]
    pub trigger_reason: Option<CsrTriggerReason>,
}

#[cfg(test)]
mod tests {
    use crate::{
        notification::{
            CsourceNotification,
            CsrTriggerReason,
            Notification,
            NotificationData,
            NotificationTrigger,
            ReceivedNotification,
            ReceivedNotificationData,
            effective_notification_triggers,
        },
        registration::ContextSourceRegistration,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use chrono::{TimeZone, Utc};
    use serde_json::{json, value::RawValue};

    /// The instant every fixture in this module is stamped with.
    fn instant() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant")
    }

    /// A subscription identifier the notification fixtures answer for.
    fn subscription_id() -> Identifier {
        "urn:ngsi-ld:Subscription:1".parse().expect("valid subscription id")
    }

    fn notification_id() -> Identifier {
        "urn:ngsi-ld:Notification:1".parse().expect("valid notification id")
    }

    fn data(text: &str) -> NotificationData {
        NotificationData::written(RawValue::from_string(text.to_owned()).expect("written JSON"))
    }

    #[test]
    fn a_notification_carries_its_fixed_type_token() {
        let (id, subscription) = (notification_id(), subscription_id());
        let notification = Notification::builder()
            .id(&id)
            .subscription_id(&subscription)
            .notified_at(instant())
            .data(data("[]"))
            .build();
        let rendered = serde_json::to_value(&notification).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("Notification"));
        pretty_assertions::assert_eq!(rendered["data"], json!([]));
        assert!(rendered.get("@context").is_none(), "{rendered}");
    }

    /// Clause 6.3.8: a body that carries the context names it first.
    #[test]
    fn a_notification_carrying_its_context_writes_it_first() {
        let (id, subscription) = (notification_id(), subscription_id());
        let context = JsonLiteral::admit(r#""https://example.org/ctx.jsonld""#).expect("a context");
        let notification = Notification::builder()
            .context(Some(&context))
            .id(&id)
            .subscription_id(&subscription)
            .notified_at(instant())
            .data(data("[]"))
            .build();
        let written = serde_json::to_string(&notification).expect("serialises");
        assert!(written.starts_with(r#"{"@context":"https://example.org/ctx.jsonld","id":"#), "{written}");
    }

    #[test]
    fn an_absent_trigger_set_uses_only_the_clause_default() {
        pretty_assertions::assert_eq!(
            effective_notification_triggers(None),
            vec![NotificationTrigger::AttributeCreated, NotificationTrigger::AttributeUpdated]
        );
    }

    #[test]
    fn entity_updated_expands_on_the_subscription_side() {
        pretty_assertions::assert_eq!(
            effective_notification_triggers(Some(&[NotificationTrigger::EntityUpdated])),
            vec![
                NotificationTrigger::AttributeCreated,
                NotificationTrigger::AttributeUpdated,
                NotificationTrigger::AttributeDeleted,
            ]
        );
    }

    #[test]
    fn precise_attribute_triggers_do_not_expand_back_to_entity_updated() {
        pretty_assertions::assert_eq!(
            effective_notification_triggers(Some(&[NotificationTrigger::AttributeUpdated])),
            vec![NotificationTrigger::AttributeUpdated]
        );
    }

    #[test]
    fn a_csource_notification_carries_its_own_type_token_and_its_registrations() {
        let (id, subscription) = (notification_id(), subscription_id());
        let registration: ContextSourceRegistration = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "endpoint": "https://source.example.com",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        }))
        .expect("a registration");
        let registrations = [registration];
        let notification = CsourceNotification::builder()
            .id(&id)
            .subscription_id(&subscription)
            .notified_at(instant())
            .data(&registrations)
            .trigger_reason(CsrTriggerReason::NewlyMatching)
            .build();
        let rendered = serde_json::to_value(&notification).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("ContextSourceNotification"));
        pretty_assertions::assert_eq!(rendered["triggerReason"], json!("newlyMatching"));
        pretty_assertions::assert_eq!(rendered["data"][0]["id"], json!("urn:ngsi-ld:ContextSourceRegistration:1"));
    }

    #[test]
    fn a_received_notification_rejects_any_other_type_token() {
        let error = serde_json::from_value::<ReceivedNotification>(json!({
            "id": "urn:ngsi-ld:Notification:1",
            "type": "CsourceNotification",
            "subscriptionId": "urn:ngsi-ld:Subscription:1",
            "notifiedAt": "2024-06-15T12:30:00Z",
            "data": []
        }))
        .expect_err("a foreign discriminator is rejected");
        assert!(error.to_string().contains("invalid value"), "{error}");
    }

    /// The written `data` is carried into the notification byte for byte, an
    /// exact decimal included.
    #[test]
    fn notification_data_keeps_an_exact_decimal() {
        let (id, subscription) = (notification_id(), subscription_id());
        let notification = Notification::builder()
            .id(&id)
            .subscription_id(&subscription)
            .notified_at(instant())
            .data(data(
                r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}}]"#,
            ))
            .build();
        let encoded = serde_json::to_string(&notification).expect("notification JSON");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
    }

    /// Table 5.3.1-1: received `data` is an Entity array or a
    /// `FeatureCollection`, and each shape reads back as the variant it was
    /// written as.
    #[test]
    fn received_notification_data_is_an_entity_array_or_a_feature_collection() {
        let entities: ReceivedNotificationData = serde_json::from_value(json!([{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}])).expect("an Entity array");
        assert!(
            matches!(entities, ReceivedNotificationData::Entities(ref list) if list.len() == 1),
            "{entities:?}"
        );

        let collection: ReceivedNotificationData = serde_json::from_value(json!({"type": "FeatureCollection", "features": []})).expect("a FeatureCollection");
        assert!(matches!(collection, ReceivedNotificationData::FeatureCollection(ref map) if map.get("type") == Some(&json!("FeatureCollection"))));
    }
}
