//! NGSI-LD subscription types (ETSI GS CIM 009 clause 5.2.12).

use crate::{
    attribute::name::AttributeName,
    constant_string,
    response_format::{JoinMode, ResponseFormat},
    rfc3339,
    uri::Uri,
};
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use mediatype::MediaTypeBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use typed_builder::TypedBuilder;
use url::Url;
use urn_rs::Urn;

constant_string!(SubscriptionType, "Subscription");

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
    /// Regex pattern to match entity ids (clause 4.5.6).
    ///
    /// Held as written: the validated pattern type lives in `andromeda-query`,
    /// which depends on this crate. See `docs/query-language-layering.md`.
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
    /// The notification target URI, absolute and carrying the scheme the
    /// transport is chosen from (`http`, `https` or `mqtt`).
    pub uri: Url,
    /// Preferred content type for the notification payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<MediaTypeBuf>,
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
    pub attributes: Option<Vec<AttributeName>>,
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
    pub pick: Option<Vec<AttributeName>>,
    /// Attributes to exclude (denylist filter).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omit: Option<Vec<AttributeName>>,
    /// Join mode for linked entity retrieval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinMode>,
    /// Maximum depth for linked entity joins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_level: Option<u32>,
    /// Timestamp of the last notification sent.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub last_notification: Option<DateTime<Utc>>,
    /// Timestamp of the last successful notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of the last failed notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub last_failure: Option<DateTime<Utc>>,
    /// Total number of notification delivery attempts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_sent: Option<i32>,
    /// Status of the last notification delivery attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<NotificationStatus>,
}

/// The entities and attributes whose changes a subscription reacts to
/// (ETSI GS CIM 009 clause 5.2.12, `entities` / `watchedAttributes` /
/// `notificationTrigger`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default))]
pub struct SubscriptionSelection {
    /// Entity selectors defining which entities trigger notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,
    /// Attribute names to watch for changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watched_attributes: Option<Vec<AttributeName>>,
    /// Conditions that trigger a notification (clause 5.3.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_trigger: Option<Vec<NotificationTrigger>>,
}

/// The query expressions that narrow a subscription's matches further
/// (ETSI GS CIM 009 clauses 4.9, 4.10, 4.13, 4.18 and 5.2.21).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default))]
pub struct SubscriptionFilters {
    /// NGSI-LD query expression to filter matching entities (clause 4.9).
    ///
    /// Held as written rather than as a parsed AST: the `q` grammar's parser
    /// lives above this crate, in the broker's query layer, so typing this
    /// member means moving that parser below the domain first. See
    /// `docs/query-language-layering.md`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Geo-spatial query expression (clause 5.2.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_q: Option<Value>,
    /// Scope query expression (clause 4.18). Held as written, for the same
    /// reason as `q`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_q: Option<String>,
    /// Context source filter expression (clause 4.13). Held as written, for the
    /// same reason as `q`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csf: Option<String>,
    /// Dataset ids to subscribe to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Vec<Urn>>,
    /// Temporal query expression (clause 5.2.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_q: Option<Value>,
}

/// How and how often a matching change is turned into a notification
/// (ETSI GS CIM 009 clauses 5.2.12 and 5.5.13).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default))]
pub struct SubscriptionDelivery {
    /// Periodic notification interval in seconds (alternative to watched attributes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_interval: Option<f64>,
    /// Minimum interval in seconds between consecutive notifications (clause 5.2.12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttling: Option<f64>,
    /// Preferred language for multi-language properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<LangTagBuf>,
    /// JSON-LD context for term expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonld_context: Option<Url>,
    /// Whether to restrict matching to local entities only (clause 5.5.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_only: Option<bool>,
}

/// Whether a subscription is running, and since when
/// (ETSI GS CIM 009 clause 5.2.12, `isActive` / `status` / `expiresAt`).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionLifecycle {
    /// Whether the subscription is currently active.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub is_active: Option<bool>,
    /// Current lifecycle status of the subscription.
    pub status: SubscriptionStatus,
    /// Expiration timestamp after which the subscription becomes inactive.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// Timestamp when the subscription was created.
    #[serde(with = "rfc3339::datetime")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the subscription was last modified.
    #[serde(with = "rfc3339::datetime")]
    pub modified_at: DateTime<Utc>,
}

/// NGSI-LD Subscription (ETSI GS CIM 009 clause 5.2.12).
///
/// A Subscription defines the conditions under which notifications are sent
/// and the delivery parameters for those notifications. The members are grouped
/// by what changes them: [`SubscriptionSelection`] follows the entity-selection
/// rules, [`SubscriptionFilters`] the query languages, [`NotificationParams`]
/// the delivery format, [`SubscriptionDelivery`] the pacing, and
/// [`SubscriptionLifecycle`] the state machine. Every group is flattened, so the
/// wire form stays the single flat object clause 5.2.12 defines.
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// Subscription identifier (clause 5.2.12: a URI, not necessarily an NGSI-LD URN).
    pub id: Uri,
    /// JSON-LD type (always `Subscription`).
    #[serde(rename = "type", default)]
    #[builder(default)]
    pub type_: SubscriptionType,
    /// Human-readable subscription name.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub subscription_name: Option<String>,
    /// Human-readable description of the subscription purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub description: Option<String>,
    /// What the subscription watches.
    #[serde(flatten)]
    #[builder(default)]
    pub selection: SubscriptionSelection,
    /// What narrows the matches further.
    #[serde(flatten)]
    #[builder(default)]
    pub filters: SubscriptionFilters,
    /// Notification delivery configuration.
    pub notification: NotificationParams,
    /// How and how often notifications are produced.
    #[serde(flatten)]
    #[builder(default)]
    pub delivery: SubscriptionDelivery,
    /// Whether the subscription is running, and since when.
    #[serde(flatten)]
    pub lifecycle: SubscriptionLifecycle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
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

    /// One subscription carrying every member clause 5.2.12 defines, so the
    /// flattened groups are proven to leave the wire form flat.
    fn fully_populated_subscription() -> serde_json::Value {
        serde_json::json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "subscriptionName": "speeding",
            "description": "vehicles over the limit",
            "entities": [{"type": "Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:.*"}],
            "watchedAttributes": ["speed"],
            "notificationTrigger": ["entityUpdated"],
            "timeInterval": 30.0,
            "q": "speed>50",
            "geoQ": {"georel": "near;maxDistance==2000"},
            "scopeQ": "/Madrid",
            "csf": "type==\"ContextSourceRegistration\"",
            "datasetId": ["urn:ngsi-ld:Dataset:1"],
            "isActive": true,
            "notification": {
                "attributes": ["speed"],
                "format": "normalized",
                "endpoint": {"uri": "https://receiver.example.com/notify", "accept": "application/ld+json"},
                "timesSent": 4,
                "status": "ok"
            },
            "expiresAt": "2030-12-31T23:59:59Z",
            "throttling": 5.0,
            "temporalQ": {"timerel": "after"},
            "lang": "en",
            "jsonldContext": "https://example.com/context.jsonld",
            "localOnly": true,
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        })
    }

    #[test]
    fn a_subscription_reserialises_to_the_flat_object_it_was_read_from() {
        let original = fully_populated_subscription();
        // clone: the original is compared against the round-tripped value below
        let subscription: Subscription = serde_json::from_value(original.clone()).expect("subscription parses");
        pretty_assertions::assert_eq!(serde_json::to_value(&subscription).expect("serialises"), original);
    }

    #[test]
    fn a_subscription_builder_produces_the_same_value_as_the_parsed_form() {
        let parsed: Subscription = serde_json::from_value(fully_populated_subscription()).expect("subscription parses");
        let built = Subscription::builder()
            .id(parsed.id.clone()) // clone: the parsed value is compared against the built one below
            .subscription_name(parsed.subscription_name.clone()) // clone: same reason
            .description(parsed.description.clone()) // clone: same reason
            .selection(parsed.selection.clone()) // clone: same reason
            .filters(parsed.filters.clone()) // clone: same reason
            .notification(parsed.notification.clone()) // clone: same reason
            .delivery(parsed.delivery.clone()) // clone: same reason
            .lifecycle(parsed.lifecycle.clone()) // clone: same reason
            .build();
        pretty_assertions::assert_eq!(
            serde_json::to_value(&built).expect("serialises"),
            serde_json::to_value(&parsed).expect("serialises")
        );
    }

    #[test]
    fn a_subscription_builder_defaults_every_group_that_carries_no_required_member() {
        let built = Subscription::builder()
            .id("urn:ngsi-ld:Subscription:2".parse::<Uri>().expect("valid id"))
            .notification(serde_json::from_value(serde_json::json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})).expect("endpoint"))
            .lifecycle(
                SubscriptionLifecycle::builder()
                    .status(SubscriptionStatus::Active)
                    .created_at(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("valid instant"))
                    .modified_at(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("valid instant"))
                    .build(),
            )
            .build();
        assert!(built.selection.entities.is_none() && built.selection.watched_attributes.is_none());
        assert!(built.filters.q.is_none() && built.filters.geo_q.is_none());
        assert!(built.delivery.throttling.is_none() && built.delivery.local_only.is_none());
        pretty_assertions::assert_eq!(built.type_, SubscriptionType);
    }

    #[test]
    fn a_subscription_endpoint_uri_is_an_absolute_url() {
        let endpoint: Endpoint =
            serde_json::from_value(serde_json::json!({"uri": "https://receiver.example.com/notify", "accept": "application/json"})).expect("parses");
        pretty_assertions::assert_eq!(endpoint.uri.scheme(), "https");
        pretty_assertions::assert_eq!(endpoint.uri.host_str(), Some("receiver.example.com"));
    }

    #[test]
    fn a_subscription_endpoint_rejects_a_relative_uri() {
        assert!(serde_json::from_value::<Endpoint>(serde_json::json!({"uri": "/notify"})).is_err());
    }

    #[test]
    fn a_subscription_endpoint_accepts_an_mqtt_target() {
        let endpoint: Endpoint = serde_json::from_value(serde_json::json!({"uri": "mqtt://broker.example.com:1883/topic"})).expect("parses");
        pretty_assertions::assert_eq!(endpoint.uri.scheme(), "mqtt");
        assert!(endpoint.accept.is_none());
    }

    #[test]
    fn a_subscription_endpoint_accept_is_a_media_type() {
        let endpoint: Endpoint =
            serde_json::from_value(serde_json::json!({"uri": "https://receiver.example.com/notify", "accept": "application/ld+json"})).expect("parses");
        let accept = endpoint.accept.expect("accept present");
        pretty_assertions::assert_eq!(accept.ty().as_str(), "application");
        pretty_assertions::assert_eq!(accept.subty().as_str(), "ld");
        pretty_assertions::assert_eq!(accept.suffix().map(|name| name.as_str().to_owned()), Some("json".to_owned()));
    }

    #[test]
    fn a_subscription_endpoint_rejects_a_malformed_accept() {
        assert!(serde_json::from_value::<Endpoint>(serde_json::json!({"uri": "https://receiver.example.com/notify", "accept": "not a media type"})).is_err());
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
