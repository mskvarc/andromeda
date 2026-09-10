//! NGSI-LD subscription types (ETSI GS CIM 009 clause 5.2.12).

use crate::{attribute_name_csv, key_value_pair::KeyValuePair, notification::NotificationTrigger};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, constant_string, positive_milliseconds::PositiveMilliseconds, rfc3339, uri::Uri};
use andromeda_ngsi_ld_query::{filter::expression::Expr, pattern::Pattern, scope::query::ScopeQuery, type_selection::grammar::TypeExprRaw};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use mediatype::MediaTypeBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::num::NonZeroU32;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use typed_builder::TypedBuilder;
use url::Url;

constant_string!(SubscriptionType, "Subscription");

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
    pub id: Option<Uri>,
    /// Regular expression the entity id must match (clause 5.2.33).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<Pattern>,
    /// Type selection expression the entity's types must satisfy (required).
    ///
    /// A clause 4.17 expression rather than a single name: a selector may
    /// alternate, as in `(Building|Tower)`, and matching used to rebuild that
    /// into an unanchored regex under which `Building` also matched an entity
    /// typed `MyBuilding`.
    #[serde(rename = "type")]
    pub type_: TypeExprRaw,
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
    /// Additional key-value pairs the delivery carries alongside the payload.
    ///
    /// Rendered as HTTP headers, as MQTT v5 user properties and as the MQTT
    /// envelope's metadata members, which is why clause 5.2.15 types them as
    /// [`KeyValuePair`] rather than as free-form JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_info: Option<Vec<KeyValuePair>>,
    /// Transport configuration the notifier reads, such as `MQTT-Version` and
    /// `MQTT-QoS`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifier_info: Option<Vec<KeyValuePair>>,
    /// Minimum period that shall elapse after a failed delivery before this
    /// endpoint is addressed again (Table 5.2.15-1 `cooldown`).
    ///
    /// This suppresses notifications rather than delaying them: "If requests
    /// are received before the cooldown period has expired, no notification is
    /// sent." It is therefore a different rule from the delivery queue's retry
    /// backoff, which re-attempts one notification that already failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown: Option<PositiveMilliseconds>,
    /// Maximum period that may elapse before one delivery to this endpoint is
    /// assumed to have failed (Table 5.2.15-1 `timeout`).
    ///
    /// The same row adds that it "only applies if the binding protocol always
    /// returns a response", which is what confines it to the HTTP binding: an
    /// MQTT publish returns nothing at `QoS` 0, and its acknowledgement window
    /// at `QoS` 1 and 2 is the binding's own (clause 7.2), not this member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<PositiveMilliseconds>,
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
    /// Maximum depth for linked entity joins (clause 4.5.23 `joinLevel`).
    ///
    /// Typed as a positive integer so `"joinLevel": 0` is rejected at
    /// deserialization with a 400, exactly as it is on the query endpoints —
    /// a zero depth names no traversal and must not be silently read as the
    /// default of one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_level: Option<NonZeroU32>,
    /// Timestamp of the last notification sent.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub last_notification: Option<DateTime<Utc>>,
    /// Timestamp of the last successful notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of the last failed notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub last_failure: Option<DateTime<Utc>>,
    /// How many times the notification has been sent (Table 5.2.14.2-1
    /// `timesSent`).
    ///
    /// Output only: "Provided by the system when querying the details of a
    /// subscription", so a value a client supplies at create or update time is
    /// overwritten by the stored counter on every read.
    ///
    /// Typed as a positive integer because the table restricts the member to
    /// "Greater than 0" at cardinality `0..1`: a counter that has not moved is
    /// an absent member, never a `0`. The stored column still floors at zero —
    /// a counter has to be somewhere before it is first advanced — and the two
    /// are different statements about different things.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_sent: Option<NonZeroU32>,
    /// How many delivery attempts drew an unsuccessful response or timed out
    /// (Table 5.2.14.2-1 `timesFailed`).
    ///
    /// Output only: "Provided by the system when querying the details of a
    /// subscription", so a value a client supplies at create or update time is
    /// overwritten by the stored counter on every read.
    ///
    /// Positive for the same reason as `timesSent`: the table restricts it to
    /// "Greater than 0" at cardinality `0..1`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_failed: Option<NonZeroU32>,
    /// Status of the last notification delivery attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<NotificationStatus>,
}

/// The entities and attributes whose changes a subscription reacts to
/// (ETSI GS CIM 009 clause 5.2.12, `entities` / `watchedAttributes` /
/// `notificationTrigger`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct SubscriptionSelection {
    /// Entity selectors defining which entities trigger notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,
    /// Attribute names to watch for changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watched_attributes: Option<Vec<AttributeName>>,
    /// Conditions that trigger a notification (clause 5.2.12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_trigger: Option<Vec<NotificationTrigger>>,
}

/// The query expressions that narrow a subscription's matches further
/// (ETSI GS CIM 009 clauses 4.9, 4.10, 4.19 and 5.2.21).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct SubscriptionFilters {
    /// NGSI-LD query expression to filter matching entities (clause 4.9).
    ///
    /// Parsed at deserialization, so a malformed filter is a rejected
    /// subscription rather than one that silently matches nothing from its
    /// first notification onward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<Expr>,
    /// Geo-spatial query expression (clause 5.2.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_q: Option<Value>,
    /// Scope query expression (clause 4.19).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_q: Option<ScopeQuery>,
    /// Context source filter expression (clause 4.9), which is a `q` filter
    /// over a registration's own properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csf: Option<Expr>,
    /// Dataset ids to subscribe to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Vec<Uri>>,
    /// Temporal query expression (clause 5.2.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_q: Option<Value>,
    /// Attributes whose `q` values are expanded through the subscription's
    /// `@context` by JSON-LD type coercion before the filter is evaluated
    /// (Table 5.2.12-1 `expandValues`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "attribute_name_csv::deserialize",
        serialize_with = "attribute_name_csv::serialize"
    )]
    pub expand_values: Option<Vec<AttributeName>>,
    /// Attributes whose values are uninterpretable as JSON-LD, so only the
    /// object keys addressed by the `q` filter are expanded through the
    /// `@context` (Table 5.2.12-1 `jsonKeys`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "attribute_name_csv::deserialize",
        serialize_with = "attribute_name_csv::serialize"
    )]
    pub json_keys: Option<Vec<AttributeName>>,
}

/// How and how often a matching change is turned into a notification
/// (ETSI GS CIM 009 clauses 5.2.12 and 5.5.13).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
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
    /// Whether single entities are assumed to be split across context sources,
    /// so filters cannot be applied before re-assembly (clause 5.2.12
    /// `splitEntities`); absent defers to the deployment default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_entities: Option<bool>,
}

/// Whether a subscription is running, and since when
/// (ETSI GS CIM 009 clause 5.2.12, `isActive` / `status` / `expiresAt`).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionLifecycle {
    /// Whether the subscription is currently active.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub is_active: Option<bool>,
    /// Current lifecycle status of the subscription.
    pub status: SubscriptionStatus,
    /// Expiration timestamp after which the subscription becomes inactive.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default = Default::default())]
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
    #[builder(default = Default::default())]
    pub type_: SubscriptionType,
    /// Human-readable subscription name.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub subscription_name: Option<String>,
    /// Human-readable description of the subscription purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub description: Option<String>,
    /// What the subscription watches.
    #[serde(flatten)]
    #[builder(default = Default::default())]
    pub selection: SubscriptionSelection,
    /// What narrows the matches further.
    #[serde(flatten)]
    #[builder(default = Default::default())]
    pub filters: SubscriptionFilters,
    /// Notification delivery configuration.
    pub notification: NotificationParams,
    /// How and how often notifications are produced.
    #[serde(flatten)]
    #[builder(default = Default::default())]
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

    /// Table 5.2.33-1: an `EntitySelector` `type` is a clause 4.17 selection
    /// string; the array form belongs to `EntityInfo` (Table 5.2.8-1) alone.
    #[test]
    fn an_entity_selector_type_is_a_selection_string() {
        let single: EntitySelector = serde_json::from_value(serde_json::json!({"type": "Vehicle"})).expect("string form parses");
        assert!(matches!(single.type_, TypeExprRaw::Type(_)));
        let alternation: EntitySelector = serde_json::from_value(serde_json::json!({"type": "(Vehicle|Car)"})).expect("alternation parses");
        assert!(matches!(&alternation.type_, TypeExprRaw::Or(alternatives) if alternatives.len() == 2));
        assert!(serde_json::from_value::<EntitySelector>(serde_json::json!({"type": ["Vehicle", "Car"]})).is_err());
        assert!(serde_json::from_value::<EntitySelector>(serde_json::json!({"type": 5})).is_err());
    }

    /// Clause 4.5.23: `joinLevel` is a positive integer. Zero names no
    /// traversal, so a subscription carrying it is rejected rather than
    /// silently read as the default of one hop.
    #[test]
    fn a_notification_join_level_of_zero_is_rejected() {
        let raw = serde_json::json!({"endpoint": {"uri": "https://example.com/notify"}, "join": "flat", "joinLevel": 0});

        let parsed = serde_json::from_value::<NotificationParams>(raw);

        assert!(parsed.is_err());
    }

    /// A positive `joinLevel` parses to exactly the depth it names.
    #[test]
    fn a_positive_notification_join_level_parses_to_that_depth() {
        let raw = serde_json::json!({"endpoint": {"uri": "https://example.com/notify"}, "join": "flat", "joinLevel": 7});

        let parsed = serde_json::from_value::<NotificationParams>(raw).expect("parses");

        assert_eq!(parsed.join_level.map(NonZeroU32::get), Some(7));
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
            "expandValues": "speed",
            "jsonKeys": "payload",
            "isActive": true,
            "notification": {
                "attributes": ["speed"],
                "format": "normalized",
                "endpoint": {"uri": "https://receiver.example.com/notify", "accept": "application/ld+json", "cooldown": 500, "timeout": 3000},
                "timesSent": 4,
                "timesFailed": 1,
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

    /// Table 5.2.15-1: `cooldown` and `timeout` are numbers of milliseconds
    /// restricted to "Greater than 0", so the endpoint keeps them typed and a
    /// non-positive or non-integer value is a parse failure rather than a
    /// silently ignored member.
    #[test]
    fn a_subscription_endpoint_carries_its_cooldown_and_timeout_in_milliseconds() {
        let endpoint: Endpoint = serde_json::from_value(serde_json::json!({
            "uri": "https://receiver.example.com/notify",
            "cooldown": 250,
            "timeout": 1500
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(endpoint.cooldown.map(PositiveMilliseconds::get), Some(250));
        pretty_assertions::assert_eq!(endpoint.timeout.map(PositiveMilliseconds::get), Some(1500));
        pretty_assertions::assert_eq!(
            serde_json::to_value(&endpoint).expect("serialises"),
            serde_json::json!({"uri": "https://receiver.example.com/notify", "cooldown": 250, "timeout": 1500})
        );
    }

    /// Table 5.2.15-1 restricts both members to "Greater than 0".
    #[test]
    fn a_subscription_endpoint_rejects_a_non_positive_cooldown_or_timeout() {
        for wire in [
            serde_json::json!({"uri": "https://receiver.example.com/notify", "cooldown": 0}),
            serde_json::json!({"uri": "https://receiver.example.com/notify", "cooldown": -1}),
            serde_json::json!({"uri": "https://receiver.example.com/notify", "cooldown": "abc"}),
            serde_json::json!({"uri": "https://receiver.example.com/notify", "timeout": 0}),
            serde_json::json!({"uri": "https://receiver.example.com/notify", "timeout": -1}),
            serde_json::json!({"uri": "https://receiver.example.com/notify", "timeout": 1.5}),
        ] {
            // clone: the rejected value is reported when the assertion fails
            assert!(serde_json::from_value::<Endpoint>(wire.clone()).is_err(), "{wire} must be rejected");
        }
    }

    /// Table 5.2.14.2-1: `timesFailed` is one of the output-only members a
    /// query or retrieve of a subscription reports.
    #[test]
    fn notification_parameters_carry_the_times_failed_counter() {
        let notification: NotificationParams = serde_json::from_value(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "timesSent": 7,
            "timesFailed": 3
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(notification.times_failed.map(NonZeroU32::get), Some(3));
        pretty_assertions::assert_eq!(notification.times_sent.map(NonZeroU32::get), Some(7));
    }

    /// Table 5.2.14.2-1 restricts both delivery counters to "Greater than 0",
    /// so a `0` is outside the data type rather than a counter at rest, and the
    /// broker never renders one.
    #[test]
    fn a_delivery_counter_of_zero_is_outside_the_notification_parameters() {
        for wire in [
            serde_json::json!({"endpoint": {"uri": "https://receiver.example.com/notify"}, "timesSent": 0}),
            serde_json::json!({"endpoint": {"uri": "https://receiver.example.com/notify"}, "timesFailed": 0}),
        ] {
            // clone: the rejected value is reported when the assertion fails
            assert!(serde_json::from_value::<NotificationParams>(wire.clone()).is_err(), "{wire} must be rejected");
        }
    }

    /// A counter the system has not moved is an absent member, which is what
    /// cardinality `0..1` means for a restriction of "Greater than 0".
    #[test]
    fn an_unmoved_delivery_counter_renders_as_an_absent_member() {
        let notification: NotificationParams =
            serde_json::from_value(serde_json::json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})).expect("parses");
        let rendered = serde_json::to_value(&notification).expect("renders");
        assert!(rendered.get("timesSent").is_none(), "{rendered}");
        assert!(rendered.get("timesFailed").is_none(), "{rendered}");
    }

    /// Table 5.2.12-1: `expandValues` and `jsonKeys` are each a single String
    /// holding a comma separated list of attribute names, not an array.
    #[test]
    fn subscription_filters_read_expand_values_and_json_keys_as_comma_separated_names() {
        let filters: SubscriptionFilters = serde_json::from_value(serde_json::json!({"expandValues": "speed,colour", "jsonKeys": "payload"})).expect("parses");
        pretty_assertions::assert_eq!(
            filters
                .expand_values
                .as_deref()
                .map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["speed", "colour"])
        );
        pretty_assertions::assert_eq!(
            filters
                .json_keys
                .as_deref()
                .map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["payload"])
        );
        pretty_assertions::assert_eq!(
            serde_json::to_value(&filters).expect("serialises"),
            serde_json::json!({"expandValues": "speed,colour", "jsonKeys": "payload"})
        );
    }

    #[test]
    fn subscription_filters_reject_an_expand_values_array_or_an_empty_member() {
        assert!(serde_json::from_value::<SubscriptionFilters>(serde_json::json!({"expandValues": ["speed"]})).is_err());
        assert!(serde_json::from_value::<SubscriptionFilters>(serde_json::json!({"jsonKeys": "payload,"})).is_err());
    }
}
