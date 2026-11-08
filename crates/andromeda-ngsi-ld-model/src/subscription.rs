//! NGSI-LD subscription types (ETSI GS CIM 009 clause 5.2.12).

pub mod entity_selector;
pub mod standing;

use crate::{
    attribute_name_csv,
    delivery_seconds,
    ignored_member,
    key_value_pair::KeyValuePair,
    notification::NotificationTrigger,
    notification_media_type::NotificationMediaType,
    query::temporal_query::TemporalQuery,
    subscription::{entity_selector::EntitySelector, standing::SubscriptionStanding},
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    constant_string,
    dereferenceable_uri::DereferenceableUri,
    identifier::Identifier,
    positive_milliseconds::PositiveMilliseconds,
    spec_version::SpecVersion,
    time::instant_serde,
};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    geo::input::GeoQueryInput,
    projection::grammar::ProjectionList,
    scope::query::ScopeQuery,
};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use langtag::LangTagBuf;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use std::num::{NonZeroU32, NonZeroU64};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use typed_builder::TypedBuilder;

constant_string!(pub SubscriptionType, "Subscription");

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

/// Notification delivery endpoint configuration (ETSI GS CIM 009 clause 5.2.15).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    /// The notification target URI, absolute and carrying the scheme the
    /// transport is chosen from (`http`, `https` or `mqtt`).
    ///
    /// Kept as the subscriber wrote it: clause 5.2.2 has the broker serve a
    /// client's members back as they were supplied, and a bare [`url::Url`]
    /// would answer with the canonical spelling instead.
    pub uri: DereferenceableUri,
    /// The media type the notification payload is written as; absent means
    /// `application/json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<NotificationMediaType>,
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
    /// The generated `createdAt` (clause 5.2.2): accepted and ignored.
    #[serde(default, skip_serializing, deserialize_with = "ignored_member::deserialize")]
    pub created_at: (),
    /// The generated `modifiedAt` (clause 5.2.2): accepted and ignored.
    #[serde(default, skip_serializing, deserialize_with = "ignored_member::deserialize")]
    pub modified_at: (),
}

/// Notification delivery parameters for a subscription (ETSI GS CIM 009 clause 5.2.14).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationParams {
    /// Attributes to include in notifications; absent means all.
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
    /// The clause 4.21 projection keeping only the members it names, one
    /// list per projection-language string the subscriber wrote.
    ///
    /// Held, and stored, with every attribute name expanded (clause 5.5.7) as
    /// the term tree [`ProjectionList`] serializes to; the clause 4.21 text a
    /// client exchanges is written and read by the subscription service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<Vec<ProjectionList>>,
    /// The clause 4.21 projection removing the members it names, held as
    /// `pick` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omit: Option<Vec<ProjectionList>>,
    /// Join mode for linked entity retrieval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinMode>,
    /// Maximum depth for linked entity joins (clause 4.5.23 `joinLevel`).
    ///
    /// Typed as a positive integer so `"joinLevel": 0` is rejected at
    /// deserialization with a 400, exactly as it is on the query endpoints.
    /// A zero depth names no traversal and must not be silently read as the
    /// default of one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_level: Option<NonZeroU32>,
    /// Timestamp of the last notification sent.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional", default)]
    pub last_notification: Option<DateTime<Utc>>,
    /// Timestamp of the last successful notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional", default)]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of the last failed notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional", default)]
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
    /// an absent member, never a `0`. The stored column still floors at zero.
    /// A counter has to be somewhere before it is first advanced, and the two
    /// are different statements about different things.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_sent: Option<NonZeroU64>,
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
    pub times_failed: Option<NonZeroU64>,
    /// Status of the last notification delivery attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<NotificationStatus>,
}

impl NotificationParams {
    /// The depth a `join` follows: `joinLevel` when given, otherwise the
    /// default of one hop (Table 5.2.14.1-1: "Default is 1").
    #[must_use]
    pub fn effective_join_level(&self) -> NonZeroU32 {
        self.join_level.unwrap_or(NonZeroU32::MIN)
    }
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
    pub geo_q: Option<GeoQueryInput>,
    /// Scope query expression (clause 4.19).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_q: Option<ScopeQuery>,
    /// Context source filter expression (clause 4.9), which is a `q` filter
    /// over a registration's own properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csf: Option<Expr>,
    /// The `datasetId`s of the Attribute instances each notification carries,
    /// `@none` naming the default instance (Table 5.2.12-1, clause 4.5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Temporal query expression (clause 5.2.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_q: Option<TemporalQuery>,
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
#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct SubscriptionDelivery {
    /// Periodic notification interval in seconds (alternative to watched attributes).
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "delivery_seconds::serialize")]
    pub time_interval: Option<f64>,
    /// Minimum interval in seconds between consecutive notifications (clause 5.2.12).
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "delivery_seconds::serialize")]
    pub throttling: Option<f64>,
    /// Preferred language for multi-language properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<LangTagBuf>,
    /// The `@context` notifications are rendered with (clause 5.8.1.4), an
    /// absolute URI (Table 5.2.12-1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonld_context: Option<IriBuf>,
    /// Whether to restrict matching to local entities only (clause 5.5.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_only: Option<bool>,
    /// Whether single entities are assumed to be split across context sources,
    /// so filters cannot be applied before re-assembly (clause 5.2.12
    /// `splitEntities`); absent defers to the deployment default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_entities: Option<bool>,
    /// The NGSI-LD specification version the Entities of a notification are
    /// amended to conform to (Table 5.2.12-1 `ngsildConformance`).
    ///
    /// Table 5.2.12-1 restricts the member to "a semantically versioned string
    /// in the form major.minor", and clause 5.8.6 has a notification "undergo a
    /// backwards compatibility operation as defined by clause 4.3.6.8" when it
    /// is set. An absent member means no amendment: there is no default version
    /// a notification falls back to.
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ngsild_conformance: Option<SpecVersion>,
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
    /// The status the subscription was last given and the instant it
    /// expires at.
    #[serde(flatten)]
    pub standing: SubscriptionStanding,
    /// Timestamp when the subscription was created.
    #[serde(with = "instant_serde::required")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the subscription was last modified.
    #[serde(with = "instant_serde::required")]
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
    pub id: Identifier,
    /// JSON-LD type (always `Subscription`, cardinality 1 in Table 5.2.12-1).
    #[serde(rename = "type")]
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
        assert_eq!(parsed.effective_join_level().get(), 7);
    }

    /// Table 5.2.14.1-1: an absent `joinLevel` follows one hop.
    #[test]
    fn an_absent_notification_join_level_follows_one_hop() {
        let raw = serde_json::json!({"endpoint": {"uri": "https://example.com/notify"}, "join": "flat"});

        let parsed = serde_json::from_value::<NotificationParams>(raw).expect("parses");

        assert_eq!(parsed.effective_join_level().get(), 1);
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
            "timeInterval": 30,
            "q": "speed>50",
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [13.4, 52.5]},
            "scopeQ": "/Madrid",
            "csf": "type==\"ContextSourceRegistration\"",
            "datasetId": ["urn:ngsi-ld:Dataset:1", "@none"],
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
            "throttling": 5,
            "temporalQ": {"timeAt": "2024-01-01T00:00:00Z", "timerel": "after"},
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
        let subscription: Subscription = serde_json::from_value(original.clone()).expect("subscription parses");
        pretty_assertions::assert_eq!(serde_json::to_value(&subscription).expect("serialises"), original);
    }

    /// Table 5.2.12-1 gives `type` cardinality 1.
    #[test]
    fn a_subscription_without_its_type_does_not_parse() {
        let mut untyped = fully_populated_subscription();
        let removed = untyped.as_object_mut().and_then(|members| members.remove("type"));
        pretty_assertions::assert_eq!(removed, Some(serde_json::json!("Subscription")));
        assert!(serde_json::from_value::<Subscription>(untyped).is_err());
    }

    #[test]
    fn a_subscription_builder_produces_the_same_value_as_the_parsed_form() {
        let parsed: Subscription = serde_json::from_value(fully_populated_subscription()).expect("subscription parses");
        let built = Subscription::builder()
            .id(parsed.id.clone())
            .subscription_name(parsed.subscription_name.clone())
            .description(parsed.description.clone())
            .selection(parsed.selection.clone())
            .filters(parsed.filters.clone())
            .notification(parsed.notification.clone())
            .delivery(parsed.delivery.clone())
            .lifecycle(parsed.lifecycle.clone())
            .build();
        pretty_assertions::assert_eq!(
            serde_json::to_value(&built).expect("serialises"),
            serde_json::to_value(&parsed).expect("serialises")
        );
    }

    #[test]
    fn a_subscription_builder_defaults_every_group_that_carries_no_required_member() {
        let built = Subscription::builder()
            .id("urn:ngsi-ld:Subscription:2".parse::<Identifier>().expect("valid id"))
            .notification(serde_json::from_value(serde_json::json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})).expect("endpoint"))
            .lifecycle(
                SubscriptionLifecycle::builder()
                    .standing(SubscriptionStanding {
                        status: SubscriptionStatus::Active,
                        expires_at: None,
                    })
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
        pretty_assertions::assert_eq!(endpoint.uri.url().scheme(), "https");
        pretty_assertions::assert_eq!(endpoint.uri.url().host_str(), Some("receiver.example.com"));
    }

    /// Table 5.2.15-1 defines the endpoint in full: a misspelt member is
    /// refused naming it, while the generated `createdAt` and `modifiedAt`
    /// (clause 5.2.2) are accepted.
    #[test]
    fn an_unknown_endpoint_member_is_refused_naming_it() {
        let error = serde_json::from_value::<Endpoint>(serde_json::json!({"uri": "https://receiver.example.com/notify", "protcol": "http"}))
            .expect_err("an unknown member is refused");
        assert!(error.to_string().contains("unknown field `protcol`"), "got {error}");
        let accepted = serde_json::from_value::<Endpoint>(
            serde_json::json!({"uri": "https://receiver.example.com/notify", "createdAt": "2024-01-01T00:00:00Z", "modifiedAt": null}),
        );
        assert!(accepted.is_ok(), "got {accepted:?}");
    }

    #[test]
    fn a_subscription_endpoint_rejects_a_relative_uri() {
        assert!(serde_json::from_value::<Endpoint>(serde_json::json!({"uri": "/notify"})).is_err());
    }

    #[test]
    fn a_subscription_endpoint_accepts_an_mqtt_target() {
        let endpoint: Endpoint = serde_json::from_value(serde_json::json!({"uri": "mqtt://broker.example.com:1883/topic"})).expect("parses");
        pretty_assertions::assert_eq!(endpoint.uri.url().scheme(), "mqtt");
        assert!(endpoint.accept.is_none());
    }

    #[test]
    fn a_subscription_endpoint_accept_is_one_of_the_notification_media_types() {
        let endpoint: Endpoint =
            serde_json::from_value(serde_json::json!({"uri": "https://receiver.example.com/notify", "accept": "application/ld+json"})).expect("parses");
        pretty_assertions::assert_eq!(endpoint.accept, Some(NotificationMediaType::JsonLd));
    }

    /// Table 5.2.15-1: `accept` "shall be one of" its three media types.
    #[test]
    fn a_subscription_endpoint_rejects_an_accept_outside_the_three() {
        for refused in ["not a media type", "application/xml"] {
            assert!(serde_json::from_value::<Endpoint>(serde_json::json!({"uri": "https://receiver.example.com/notify", "accept": refused})).is_err());
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
        pretty_assertions::assert_eq!(notification.times_failed.map(NonZeroU64::get), Some(3));
        pretty_assertions::assert_eq!(notification.times_sent.map(NonZeroU64::get), Some(7));
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
