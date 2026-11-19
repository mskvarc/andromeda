//! A Subscription as a client reads it (ETSI GS CIM 009 clauses 4.8, 5.2.12
//! and 5.2.14.2).
//!
//! Two members are generated with a default and are written back only when
//! they differ from it: `isActive`, which Table 5.2.12-2 derives from
//! `status`, and `localOnly`, whose absence means `false`, which the service
//! leaves absent. The clause 4.8 system timestamps are withheld unless
//! `sysAttrs` asks for them. Everything else clause 5.2.12 defines stays,
//! `jsonldContext` included: clause 5.8.3.5 returns "the subscription details
//! as mandated by clause 5.2.12", and clause 5.11.4.5 says the same of a
//! Context Source Registration Subscription, whose resource clause 5.11.2.3
//! takes from that very data type. The two resources are therefore written by
//! one rule.
//!
//! The view borrows the presentation, so the Subscription is written straight
//! from its typed form with no document built beside it. The members that hold
//! names are written from the names in the reader's terms, `pick` and `omit` as
//! the clause 4.21 strings a client sent, and the lifecycle members honour
//! `sysAttrs`.

use crate::subscription::{filters_view::FiltersView, selection_view::SelectionView};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::instant_serde};
use andromeda_ngsi_ld_model::{
    subscription::{Endpoint, NotificationParams, NotificationStatus, SubscriptionDelivery, SubscriptionLifecycle, SubscriptionStatus, SubscriptionType},
    subscription_presentation::SubscriptionPresentation,
    written_subscription_terms::WrittenSubscriptionTerms,
};
use andromeda_ngsi_ld_query::projection::written_projection_list::WrittenProjectionList;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};
use std::num::{NonZeroU32, NonZeroU64};

mod filters_view;
mod selection_view;

/// The projection lists of one `pick` or `omit` member, written as the clause
/// 4.21 strings they were sent as.
#[derive(Clone, Copy, Debug)]
struct ProjectionStrings<'a>(&'a [WrittenProjectionList]);

// A derive would write each list as the term tree the subscription stores.
impl Serialize for ProjectionStrings<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(WrittenProjectionList::to_string))
    }
}

/// The `notification` member of a Subscription as a client reads it (clause
/// 5.2.14).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotificationView<'a> {
    /// Attributes to include in notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<&'a [AttributeName]>,
    /// Representation format for notification payloads.
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<ResponseFormat>,
    /// Target endpoint for notification delivery.
    endpoint: &'a Endpoint,
    /// Whether system-generated attributes are included.
    #[serde(skip_serializing_if = "Option::is_none")]
    sys_attrs: Option<bool>,
    /// Whether previous attribute values are included.
    #[serde(skip_serializing_if = "Option::is_none")]
    show_changes: Option<bool>,
    /// The clause 4.21 projection keeping only the members it names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pick: Option<ProjectionStrings<'a>>,
    /// The clause 4.21 projection removing the members it names.
    #[serde(skip_serializing_if = "Option::is_none")]
    omit: Option<ProjectionStrings<'a>>,
    /// Join mode for linked entity retrieval.
    #[serde(skip_serializing_if = "Option::is_none")]
    join: Option<JoinMode>,
    /// Maximum depth for linked entity joins.
    #[serde(skip_serializing_if = "Option::is_none")]
    join_level: Option<NonZeroU32>,
    /// Timestamp of the last notification sent.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional")]
    last_notification: Option<DateTime<Utc>>,
    /// Timestamp of the last successful notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional")]
    last_success: Option<DateTime<Utc>>,
    /// Timestamp of the last failed notification delivery.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional")]
    last_failure: Option<DateTime<Utc>>,
    /// How many times the notification has been sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    times_sent: Option<NonZeroU64>,
    /// How many delivery attempts drew an unsuccessful response or timed out.
    #[serde(skip_serializing_if = "Option::is_none")]
    times_failed: Option<NonZeroU64>,
    /// Status of the last notification delivery attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<NotificationStatus>,
}

impl<'a> NotificationView<'a> {
    /// The notification parameters as a client reads them.
    fn of(notification: &'a NotificationParams, terms: &'a WrittenSubscriptionTerms) -> NotificationView<'a> {
        NotificationView {
            attributes: terms.attributes.as_deref(),
            format: notification.format,
            endpoint: &notification.endpoint,
            sys_attrs: notification.sys_attrs,
            show_changes: notification.show_changes,
            pick: terms.pick.as_deref().map(ProjectionStrings),
            omit: terms.omit.as_deref().map(ProjectionStrings),
            join: notification.join,
            join_level: notification.join_level,
            last_notification: notification.last_notification,
            last_success: notification.last_success,
            last_failure: notification.last_failure,
            times_sent: notification.times_sent,
            times_failed: notification.times_failed,
            status: notification.status,
        }
    }
}

/// Whether the clause 4.8 system timestamps are written.
const fn timestamp(instant: DateTime<Utc>, sys_attrs: SysAttrsInclusion) -> Option<DateTime<Utc>> {
    match sys_attrs {
        SysAttrsInclusion::Include => Some(instant),
        SysAttrsInclusion::Strip => None,
    }
}

/// The lifecycle members of a Subscription as a client reads them.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleView {
    /// Written only for a paused subscription, which is what `isActive` being
    /// `false` says (Table 5.2.12-2).
    #[serde(skip_serializing_if = "Option::is_none")]
    is_active: Option<bool>,
    /// Current lifecycle status of the subscription.
    status: SubscriptionStatus,
    /// Expiration timestamp after which the subscription becomes inactive.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional")]
    expires_at: Option<DateTime<Utc>>,
    /// Timestamp when the subscription was created, when `sysAttrs` asks.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional")]
    created_at: Option<DateTime<Utc>>,
    /// Timestamp when the subscription was last modified, when `sysAttrs` asks.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional")]
    modified_at: Option<DateTime<Utc>>,
}

impl LifecycleView {
    /// The lifecycle members as a client reads them under `sys_attrs`.
    ///
    /// Clause 4.8: `sysAttrs` governs only the system timestamps. Hiding a
    /// member that equals its default is a separate policy it does not switch
    /// off, so `sysAttrs` must not resurrect a defaulted `isActive`.
    fn of(lifecycle: &SubscriptionLifecycle, sys_attrs: SysAttrsInclusion) -> LifecycleView {
        LifecycleView {
            is_active: lifecycle.is_active.filter(|active| !active),
            status: lifecycle.standing.status,
            expires_at: lifecycle.standing.expires_at,
            created_at: timestamp(lifecycle.created_at, sys_attrs),
            modified_at: timestamp(lifecycle.modified_at, sys_attrs),
        }
    }
}

/// A Subscription written as clause 5.2.12 and a client's `sysAttrs` define it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionView<'a> {
    /// Subscription identifier.
    id: &'a Identifier,
    /// JSON-LD type (always `Subscription`).
    #[serde(rename = "type")]
    type_: SubscriptionType,
    /// Human-readable subscription name.
    #[serde(skip_serializing_if = "Option::is_none")]
    subscription_name: Option<&'a str>,
    /// Human-readable description of the subscription purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    /// What the subscription watches.
    #[serde(flatten)]
    selection: SelectionView<'a>,
    /// What narrows the matches further.
    #[serde(flatten)]
    filters: FiltersView<'a>,
    /// Notification delivery configuration.
    notification: NotificationView<'a>,
    /// How and how often notifications are produced.
    #[serde(flatten)]
    delivery: &'a SubscriptionDelivery,
    /// Whether the subscription is running, and since when.
    #[serde(flatten)]
    lifecycle: LifecycleView,
}

impl<'a> SubscriptionView<'a> {
    /// The view of `subscription` under `sys_attrs`.
    #[must_use]
    pub fn of(presentation: &'a SubscriptionPresentation, sys_attrs: SysAttrsInclusion) -> SubscriptionView<'a> {
        let subscription = &presentation.subscription;
        let terms = &presentation.terms;
        SubscriptionView {
            id: &subscription.id,
            type_: subscription.type_,
            subscription_name: subscription.subscription_name.as_deref(),
            description: subscription.description.as_deref(),
            selection: SelectionView {
                entities: terms.entities.as_deref(),
                watched_attributes: terms.watched_attributes.as_deref(),
                notification_trigger: subscription.selection.notification_trigger.as_deref(),
            },
            filters: FiltersView {
                q: terms.q.as_ref(),
                geo_q: terms.geo_q.as_ref(),
                scope_q: subscription.filters.scope_q.as_ref(),
                csf: terms.csf.as_ref(),
                dataset_id: subscription.filters.dataset_id.as_deref(),
                temporal_q: subscription.filters.temporal_q.as_ref(),
                expand_values: &terms.expand_values,
                json_keys: &terms.json_keys,
            },
            notification: NotificationView::of(&subscription.notification, terms),
            delivery: &subscription.delivery,
            lifecycle: LifecycleView::of(&subscription.lifecycle, sys_attrs),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription::SubscriptionView;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::{
        subscription::Subscription,
        subscription_presentation::SubscriptionPresentation,
        written_subscription_terms::WrittenSubscriptionTerms,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use serde::de::DeserializeOwned;
    use serde_json::{Map, Value, json};

    /// The member `key` of `members`, taken out and read as a `T`.
    fn taken<T: DeserializeOwned>(members: &mut Map<String, Value>, key: &str) -> Option<T> {
        members.remove(key).map(|member| serde_json::from_value(member).expect("a readable member"))
    }

    /// The attribute names the comma-separated member `key` lists.
    fn listed(members: &mut Map<String, Value>, key: &str) -> Option<Vec<AttributeName>> {
        taken::<String>(members, key).map(|text| text.split(',').map(|name| name.parse().expect("a valid attribute name")).collect())
    }

    /// A Subscription whose members are `members`, over the minimum one holds, its names in the terms of the reader.
    fn subscription(members: Value) -> SubscriptionPresentation {
        let mut wire = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "notification": {"endpoint": {"uri": "https://example.org"}},
            "status": "active",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z"
        });
        if let (Some(target), Value::Object(extra)) = (wire.as_object_mut(), members) {
            target.extend(extra);
        }
        let mut terms = WrittenSubscriptionTerms::default();
        if let Some(members) = wire.as_object_mut() {
            terms.entities = taken(members, "entities");
            terms.watched_attributes = taken(members, "watchedAttributes");
            terms.q = taken::<String>(members, "q").map(|text| text.parse().expect("a valid q"));
            terms.csf = taken::<String>(members, "csf").map(|text| text.parse().expect("a valid csf"));
            terms.geo_q = taken(members, "geoQ");
            terms.expand_values = listed(members, "expandValues");
            terms.json_keys = listed(members, "jsonKeys");
            if let Some(Value::Object(notification)) = members.get_mut("notification") {
                terms.attributes = taken(notification, "attributes");
                terms.pick = taken(notification, "pick");
                terms.omit = taken(notification, "omit");
            }
        }
        let subscription: Subscription = serde_json::from_value(wire).expect("a valid subscription");
        SubscriptionPresentation { subscription, terms }
    }

    /// `members` as a client reads the Subscription under `sys_attrs`.
    fn rendered(members: Value, sys_attrs: SysAttrsInclusion) -> Value {
        serde_json::to_value(SubscriptionView::of(&subscription(members), sys_attrs)).expect("the view writes")
    }

    #[test]
    fn a_subscription_view_strips_sys_attrs_when_not_included() {
        let rendered = rendered(json!({"isActive": true, "entities": [{"type": "Vehicle"}]}), SysAttrsInclusion::Strip);
        assert!(rendered.get("createdAt").is_none());
        assert!(rendered.get("modifiedAt").is_none());
        assert!(rendered.get("isActive").is_none());
        assert!(rendered.get("localOnly").is_none());
        assert!(rendered.get("entities").is_some());
    }

    /// Clause 4.8: `sysAttrs` keeps the timestamps, but default-value hiding
    /// is a separate policy it does not switch off, so an `isActive: true`
    /// default is still omitted.
    #[test]
    fn a_subscription_view_keeps_timestamps_but_still_hides_defaults_when_sys_attrs_included() {
        let rendered = rendered(json!({"isActive": true}), SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(rendered.get("createdAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(rendered.get("modifiedAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        assert!(rendered.get("isActive").is_none(), "the isActive default is still hidden: {rendered}");
    }

    #[test]
    fn a_subscription_view_keeps_is_active_when_false() {
        let rendered = rendered(json!({"isActive": false, "status": "paused"}), SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(rendered.get("isActive").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn a_subscription_view_keeps_local_only_when_true() {
        let rendered = rendered(json!({"localOnly": true}), SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(rendered.get("localOnly").and_then(Value::as_bool), Some(true));
    }

    /// Clause 5.11.4.5 returns the clause 5.2.12 details of a Context Source
    /// Registration Subscription: `jsonldContext` and the notification
    /// runtime are members of it.
    #[test]
    fn a_subscription_view_keeps_its_jsonld_context_and_notification_runtime() {
        let rendered = rendered(
            json!({
                "jsonldContext": "https://example.org/ctx",
                "notification": {
                    "endpoint": {"uri": "https://example.org"},
                    "timesSent": 42,
                    "lastNotification": "2025-01-01T00:00:00Z",
                    "lastSuccess": "2025-01-01T00:00:00Z",
                    "lastFailure": "2025-01-01T00:00:00Z"
                }
            }),
            SysAttrsInclusion::Strip,
        );
        pretty_assertions::assert_eq!(rendered.get("jsonldContext").and_then(Value::as_str), Some("https://example.org/ctx"));
        let notification = rendered.get("notification").expect("notification");
        assert!(notification.get("endpoint").is_some());
        pretty_assertions::assert_eq!(notification.get("timesSent").and_then(Value::as_i64), Some(42));
        assert!(notification.get("lastNotification").is_some());
        assert!(notification.get("lastSuccess").is_some());
        assert!(notification.get("lastFailure").is_some());
    }

    /// Table 5.2.14.1-1: `pick` and `omit` are written as the clause 4.21
    /// strings a client sent, one per stored list.
    #[test]
    fn the_stored_projection_trees_are_written_as_clause_4_21_strings() {
        let rendered = rendered(
            json!({
                "notification": {
                    "endpoint": {"uri": "https://example.org"},
                    "pick": [[{"name": "id", "linked": []}], [{"name": "owner", "linked": [{"name": "name", "linked": []}]}]],
                    "omit": [[{"name": "scope", "linked": []}, {"name": "colour", "linked": []}]]
                }
            }),
            SysAttrsInclusion::Strip,
        );
        pretty_assertions::assert_eq!(rendered["notification"]["pick"], json!(["id", "owner{name}"]));
        pretty_assertions::assert_eq!(rendered["notification"]["omit"], json!(["scope,colour"]));
    }

    /// Clause 5.2.12: the members the Subscription holds are written flat, as
    /// the resource itself writes them.
    #[test]
    fn the_view_writes_the_flat_object_the_resource_writes() {
        let members = json!({
            "subscriptionName": "speeding",
            "entities": [{"type": "Vehicle"}],
            "watchedAttributes": ["speed"],
            "q": "speed>50",
            "timeInterval": 30,
            "lang": "en",
            "expiresAt": "2030-12-31T23:59:59Z"
        });
        let resource = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "subscriptionName": "speeding",
            "entities": [{"type": "Vehicle"}],
            "watchedAttributes": ["speed"],
            "q": "speed>50",
            "notification": {"endpoint": {"uri": "https://example.org"}},
            "timeInterval": 30,
            "lang": "en",
            "status": "active",
            "expiresAt": "2030-12-31T23:59:59Z",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z"
        });
        pretty_assertions::assert_eq!(rendered(members, SysAttrsInclusion::Include), resource);
    }
}
