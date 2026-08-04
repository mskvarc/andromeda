//! NGSI-LD subscription types (ETSI GS CIM 009 clause 5.2.12).

use crate::{
    response_format::{JoinMode, ResponseFormat},
    serde_helpers,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// Trigger conditions that cause a notification to be sent (ETSI GS CIM 009 clause 5.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
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

/// Lifecycle status of a subscription (ETSI GS CIM 009 clause 5.2.12, status member).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, EnumIter, IntoStaticStr)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum SubscriptionStatus {
    /// The subscription is active and will generate notifications.
    Active,
    /// The subscription is paused and will not generate notifications.
    Paused,
    /// The subscription has passed its expiration time.
    Expired,
}

/// Status of the last notification delivery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum NotificationStatus {
    /// The last notification was delivered successfully.
    Ok,
    /// The last notification delivery failed.
    Failed,
}

/// Entity selector for subscriptions (ETSI GS CIM 009 clause 5.2.33).
///
/// Selects entities by type and optionally by id or id pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySelector {
    /// Exact entity id to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Regex pattern to match entity ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<String>,
    /// Entity type to match (required).
    #[serde(rename = "type")]
    pub type_: String,
}

/// Notification delivery endpoint configuration (ETSI GS CIM 009 clause 5.2.15).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    /// The notification target URI (HTTP or MQTT).
    pub uri: String,
    /// Preferred content type for the notification payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    /// Additional headers to include in HTTP notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_info: Option<Vec<Value>>,
    /// MQTT-specific configuration (version, QoS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifier_info: Option<Vec<Value>>,
}

/// Notification delivery parameters for a subscription (ETSI GS CIM 009 clause 5.2.14).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationParams {
    /// Attributes to include in notifications (empty means all).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Vec<String>>,
    /// Representation format for notification payloads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ResponseFormat>,
    /// Target endpoint for notification delivery.
    pub endpoint: Endpoint,
    /// Whether to include system-generated attributes (createdAt, modifiedAt).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sys_attrs: Option<bool>,
    /// Whether to include previous attribute values in notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_changes: Option<bool>,
    /// Attributes to include (allowlist filter).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<Vec<String>>,
    /// Attributes to exclude (denylist filter).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omit: Option<Vec<String>>,
    /// Join mode for linked entity retrieval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinMode>,
    /// Maximum depth for linked entity joins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_level: Option<u32>,
    /// Timestamp of the last notification sent.
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub last_notification: Option<DateTime<Utc>>,
    /// Timestamp of the last successful notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of the last failed notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub last_failure: Option<DateTime<Utc>>,
    /// Total number of notification delivery attempts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_sent: Option<i32>,
    /// Status of the last notification delivery attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<NotificationStatus>,
}

fn default_subscription_type() -> String {
    "Subscription".to_string()
}

/// NGSI-LD Subscription (ETSI GS CIM 009 clause 5.2.12).
///
/// A Subscription defines the conditions under which notifications are sent
/// and the delivery parameters for those notifications. Supports entity
/// filtering by type, id pattern, attribute watches, geo-queries, and
/// temporal queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// Subscription identifier.
    pub id: String,
    /// JSON-LD type (always "Subscription").
    #[serde(rename = "type", default = "default_subscription_type")]
    pub type_: String,
    /// Human-readable subscription name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_name: Option<String>,
    /// Human-readable description of the subscription purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Entity selectors defining which entities trigger notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,
    /// Attribute names to watch for changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watched_attributes: Option<Vec<String>>,
    /// Conditions that trigger a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_trigger: Option<Vec<NotificationTrigger>>,
    /// Periodic notification interval in seconds (alternative to watched attributes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_interval: Option<f64>,
    /// NGSI-LD query expression to filter matching entities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Geo-spatial query expression (clause 5.2.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_q: Option<Value>,
    /// Scope query expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_q: Option<String>,
    /// Context source filter expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csf: Option<String>,
    /// Dataset ids to subscribe to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Vec<String>>,
    /// Whether the subscription is currently active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// Notification delivery configuration.
    pub notification: NotificationParams,
    /// Expiration timestamp after which the subscription becomes inactive.
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// Minimum interval in seconds between consecutive notifications (clause 5.2.12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttling: Option<f64>,
    /// Temporal query expression (clause 5.2.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_q: Option<Value>,
    /// Preferred language for multi-language properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// JSON-LD context for term expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonld_context: Option<String>,
    /// Whether to restrict matching to local entities only (clause 5.5.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_only: Option<bool>,
    /// Current lifecycle status of the subscription.
    pub status: SubscriptionStatus,
    /// Timestamp when the subscription was created.
    #[serde(with = "serde_helpers::rfc3339")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the subscription was last modified.
    #[serde(with = "serde_helpers::rfc3339")]
    pub modified_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    /// Labels of the PostgreSQL `subscription_status` enum, verbatim from
    /// `migrations/tenant/0003_types.sql`.
    const SUBSCRIPTION_STATUS_LABELS: [(&str, SubscriptionStatus); 3] = [
        ("active", SubscriptionStatus::Active),
        ("paused", SubscriptionStatus::Paused),
        ("expired", SubscriptionStatus::Expired),
    ];

    #[test]
    fn subscription_status_parses_from_its_postgres_label() {
        for (label, expected) in SUBSCRIPTION_STATUS_LABELS {
            pretty_assertions::assert_eq!(label.parse::<SubscriptionStatus>().expect(label), expected);
        }
    }

    #[test]
    fn subscription_status_renders_the_label_postgres_expects() {
        for (label, variant) in SUBSCRIPTION_STATUS_LABELS {
            pretty_assertions::assert_eq!(variant.to_string(), label);
            pretty_assertions::assert_eq!(<&str>::from(variant), label);
        }
    }

    #[test]
    fn subscription_status_round_trips_through_its_label_for_every_variant() {
        // Exhaustive over `EnumIter`: this is what replaces the type-level
        // guarantee `sqlx::Type` used to give. A variant added without a
        // matching migration fails here rather than in a production query.
        for variant in SubscriptionStatus::iter() {
            pretty_assertions::assert_eq!(variant.to_string().parse::<SubscriptionStatus>().expect("round trip"), variant);
            assert!(
                SUBSCRIPTION_STATUS_LABELS.iter().any(|(_, known)| *known == variant),
                "{variant:?} has no PostgreSQL label in subscription_status"
            );
        }
    }

    #[test]
    fn an_unknown_subscription_status_label_is_a_parse_error_not_a_panic() {
        assert!("not-a-status".parse::<SubscriptionStatus>().is_err());
    }

    #[test]
    fn notification_trigger_display_and_parse_round_trip() {
        for variant in [
            NotificationTrigger::EntityCreated,
            NotificationTrigger::EntityUpdated,
            NotificationTrigger::EntityDeleted,
            NotificationTrigger::AttributeCreated,
            NotificationTrigger::AttributeUpdated,
            NotificationTrigger::AttributeDeleted,
        ] {
            let s = variant.to_string();
            let parsed = NotificationTrigger::from_str(&s).unwrap();
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn subscription_status_display_and_parse_round_trip() {
        for status in [SubscriptionStatus::Active, SubscriptionStatus::Paused, SubscriptionStatus::Expired] {
            let s = status.to_string();
            let parsed = SubscriptionStatus::from_str(&s).unwrap();
            pretty_assertions::assert_eq!(parsed, status, "round-trip failed for {status}");
        }
    }

    #[test]
    fn notification_status_display_and_parse_round_trip() {
        for status in [NotificationStatus::Ok, NotificationStatus::Failed] {
            let s = status.to_string();
            let parsed = NotificationStatus::from_str(&s).unwrap();
            pretty_assertions::assert_eq!(parsed, status, "round-trip failed for {status}");
        }
    }
}
