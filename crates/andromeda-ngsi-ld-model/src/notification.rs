//! Change events and the notification payloads they produce (ETSI GS CIM 009 clause 5.3).

use andromeda_ngsi_ld_lexicon::{constant_string, uri::Uri};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
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
#[derive(Clone, Debug)]
pub struct SysAttrsSnapshot {
    /// Creation timestamp the entity carried.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp the entity carried.
    pub modified_at: DateTime<Utc>,
    /// Expiration timestamp the entity carried, if any.
    pub expires_at: Option<DateTime<Utc>>,
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
    #[builder(default = Default::default())]
    pub type_: NotificationType,
    /// Subscription this notification answers.
    pub subscription_id: Uri,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The entity representations that triggered it.
    #[builder(default = Default::default())]
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
    /// Always `ContextSourceNotification`.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: CsourceNotificationType,
    /// Subscription this notification answers.
    pub subscription_id: Uri,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The registration representations that triggered it.
    #[builder(default = Default::default())]
    pub data: Vec<Value>,
    /// Why the notification fired.
    #[builder(default = Default::default())]
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
    use crate::notification::{CsourceNotification, CsrTriggerReason, Notification, NotificationTrigger, effective_notification_triggers};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

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
    fn a_csource_notification_carries_its_own_type_token() {
        let notification = CsourceNotification::builder()
            .id("urn:ngsi-ld:CsourceNotification:1".parse::<Uri>().expect("valid notification id"))
            .subscription_id(subscription_id())
            .notified_at(instant())
            .trigger_reason(Some(CsrTriggerReason::NewlyMatching))
            .build();
        let rendered = serde_json::to_value(&notification).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("ContextSourceNotification"));
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
    fn notification_data_keeps_an_exact_decimal() {
        let entity = serde_json::from_str(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}}"#)
            .expect("exact entity JSON");
        let notification = Notification::builder()
            .id("urn:ngsi-ld:Notification:1".parse::<Uri>().expect("valid notification id"))
            .subscription_id(subscription_id())
            .notified_at(instant())
            .data(vec![entity])
            .build();
        let encoded = serde_json::to_string(&notification).expect("notification JSON");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
    }
}
