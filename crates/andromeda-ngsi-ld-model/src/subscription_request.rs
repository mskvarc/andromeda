//! The body of a request that creates or updates a Subscription or a Context
//! Source Registration Subscription (ETSI GS CIM 009 clauses 5.8.1, 5.8.2,
//! 5.11.2 and 5.11.3).
//!
//! Clause 5.8.1 creates a Subscription from the members of Table 5.2.12-1 and
//! clause 5.8.2 updates one with a Subscription Fragment, which is the
//! same set of members with each of them optional. The one type therefore
//! stands for both bodies: what a create requires is a rule of the operation
//! that reads it, and a member the body leaves out is the member the update
//! leaves alone.
//!
//! Clause 5.8.2.4 modifies the target "as mandated by clause 5.5.8", where "for
//! each member of the Fragment, whose value is an NGSI-LD Null, contained by the
//! target, the target member is deleted". Every member Table 5.2.12-1 gives
//! cardinality `0..1` is therefore a [`Clearable`] member that tells an absent
//! member from a cleared one: a JSON `null` clears it, and so does the URI
//! `urn:ngsi-ld:null` (an array of one of them) for the members that take URIs
//! (clause 4.5.0). The members of cardinality `1` (`type` and `notification`)
//! and `id`, which "cannot be later modified in update operations", cannot be
//! deleted, so a `null` for one of them is a value outside its data type
//! (clause 5.5.4). The members the broker generates (`status`, `createdAt`,
//! `modifiedAt`) are not part of the body and are ignored when a client sends
//! them.

use crate::{
    attribute_name_csv,
    clearable::Clearable,
    clearable_parsed,
    clearable_timestamp,
    delivery_seconds,
    ngsi_ld_null_member,
    non_null_member,
    notification::NotificationTrigger,
    notification_request::NotificationRequest,
    query::temporal_query::TemporalQuery,
    request_body::RequestBody,
    subscription::{EntitySelector, SubscriptionType},
    subscription_member::SubscriptionMember,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, spec_version::SpecVersion};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, filter::expression::Expr, geo::input::GeoQueryInput, scope::query::ScopeQuery};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use langtag::LangTagBuf;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// A Subscription create body or update fragment (Table 5.2.12-1).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct SubscriptionRequest {
    /// Subscription identifier (clause 5.2.12: a URI, not necessarily an
    /// NGSI-LD URN); the broker mints one when a create names none.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_null_member::deserialize")]
    pub id: Option<Identifier>,
    /// JSON-LD type, which can only be `Subscription`. `None` when the body
    /// leaves it out: a create refuses that (Table 5.2.12-1 gives it
    /// cardinality 1), an update fragment may do it.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null_member::deserialize"
    )]
    pub type_: Option<SubscriptionType>,
    /// Human-readable subscription name.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub subscription_name: Clearable<String>,
    /// Human-readable description of the subscription purpose.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub description: Clearable<String>,
    /// Entity selectors defining which entities trigger notifications.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub entities: Clearable<Vec<EntitySelector>>,
    /// Attribute names to watch for changes.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "ngsi_ld_null_member::deserialize_uri_array"
    )]
    pub watched_attributes: Clearable<Vec<AttributeName>>,
    /// Conditions that trigger a notification.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub notification_trigger: Clearable<Vec<NotificationTrigger>>,
    /// Periodic notification interval in seconds.
    #[serde(default, skip_serializing_if = "Clearable::is_absent", serialize_with = "delivery_seconds::clearable::serialize")]
    pub time_interval: Clearable<f64>,
    /// NGSI-LD query expression to filter matching entities (clause 4.9).
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub q: Clearable<Expr>,
    /// Geo-spatial query expression (clause 5.2.13); `null` removes it.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub geo_q: Clearable<GeoQueryInput>,
    /// Scope query expression (clause 4.19).
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub scope_q: Clearable<ScopeQuery>,
    /// Context source filter expression (clause 4.9).
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub csf: Clearable<Expr>,
    /// The `datasetId`s of the Attribute instances each notification carries.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "ngsi_ld_null_member::deserialize_uri_array"
    )]
    pub dataset_id: Clearable<Vec<DatasetSelector>>,
    /// Temporal query expression (clause 5.2.21); `null` removes it.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub temporal_q: Clearable<TemporalQuery>,
    /// Attributes whose `q` values are expanded by JSON-LD type coercion
    /// (Table 5.2.12-1 `expandValues`); `null` removes the list.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "attribute_name_csv::clearable::deserialize",
        serialize_with = "attribute_name_csv::clearable::serialize"
    )]
    pub expand_values: Clearable<Vec<AttributeName>>,
    /// Attributes whose values are uninterpretable as JSON-LD (Table 5.2.12-1
    /// `jsonKeys`); `null` removes the list.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "attribute_name_csv::clearable::deserialize",
        serialize_with = "attribute_name_csv::clearable::serialize"
    )]
    pub json_keys: Clearable<Vec<AttributeName>>,
    /// Notification delivery configuration.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_null_member::deserialize")]
    pub notification: Option<NotificationRequest>,
    /// Minimum interval in seconds between consecutive notifications.
    #[serde(default, skip_serializing_if = "Clearable::is_absent", serialize_with = "delivery_seconds::clearable::serialize")]
    pub throttling: Clearable<f64>,
    /// Expiration timestamp after which the subscription becomes inactive.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "clearable_timestamp::deserialize",
        serialize_with = "clearable_timestamp::serialize"
    )]
    pub expires_at: Clearable<DateTime<Utc>>,
    /// Whether the subscription is active.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub is_active: Clearable<bool>,
    /// Preferred language for multi-language properties.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub lang: Clearable<LangTagBuf>,
    /// The `@context` notifications are rendered with (clause 5.8.1.4), an
    /// absolute URI.
    #[serde(default, skip_serializing_if = "Clearable::is_absent", deserialize_with = "ngsi_ld_null_member::deserialize_uri")]
    pub jsonld_context: Clearable<IriBuf>,
    /// Whether to restrict matching to local entities only (clause 5.5.13).
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub local_only: Clearable<bool>,
    /// Whether single entities are assumed to be split across context sources.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub split_entities: Clearable<bool>,
    /// The NGSI-LD specification version the Entities of a notification are
    /// amended to conform to (Table 5.2.12-1 `ngsildConformance`), a string of
    /// the form `major.minor`.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "clearable_parsed::deserialize",
        serialize_with = "clearable_parsed::serialize"
    )]
    pub ngsild_conformance: Clearable<SpecVersion>,
}

impl RequestBody for SubscriptionRequest {}

impl SubscriptionRequest {
    /// The first member, in the order of Table 5.2.12-1, which the body wrote
    /// as a null: a JSON `null` or, for a member that takes a URI, the NGSI-LD
    /// Null `urn:ngsi-ld:null` (clause 4.5.0).
    ///
    /// Clause 5.5.8 gives the Null the meaning "the target member is deleted"
    /// in an update; a create has no member to delete, and clause 5.2.1 makes
    /// an NGSI-LD Null elsewhere `BadRequestData`.
    #[must_use]
    pub fn null_member(&self) -> Option<SubscriptionMember> {
        [
            (SubscriptionMember::SubscriptionName, self.subscription_name.is_null()),
            (SubscriptionMember::Description, self.description.is_null()),
            (SubscriptionMember::Entities, self.entities.is_null()),
            (SubscriptionMember::WatchedAttributes, self.watched_attributes.is_null()),
            (SubscriptionMember::NotificationTrigger, self.notification_trigger.is_null()),
            (SubscriptionMember::TimeInterval, self.time_interval.is_null()),
            (SubscriptionMember::Q, self.q.is_null()),
            (SubscriptionMember::GeoQ, self.geo_q.is_null()),
            (SubscriptionMember::ScopeQ, self.scope_q.is_null()),
            (SubscriptionMember::Csf, self.csf.is_null()),
            (SubscriptionMember::DatasetId, self.dataset_id.is_null()),
            (SubscriptionMember::TemporalQ, self.temporal_q.is_null()),
            (SubscriptionMember::ExpandValues, self.expand_values.is_null()),
            (SubscriptionMember::JsonKeys, self.json_keys.is_null()),
            (SubscriptionMember::Throttling, self.throttling.is_null()),
            (SubscriptionMember::ExpiresAt, self.expires_at.is_null()),
            (SubscriptionMember::IsActive, self.is_active.is_null()),
            (SubscriptionMember::Lang, self.lang.is_null()),
            (SubscriptionMember::JsonldContext, self.jsonld_context.is_null()),
            (SubscriptionMember::LocalOnly, self.local_only.is_null()),
            (SubscriptionMember::SplitEntities, self.split_entities.is_null()),
            (SubscriptionMember::NgsildConformance, self.ngsild_conformance.is_null()),
        ]
        .into_iter()
        .find_map(|(member, written_null)| written_null.then_some(member))
    }

    /// Whether the body carries no member to apply beyond its `id` and
    /// `type`, as the empty Fragment `{}` does: clause 5.5.8 acts only for
    /// each member of the Fragment, so such an update modifies nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        let SubscriptionRequest {
            id: _,
            type_: _,
            subscription_name,
            description,
            entities,
            watched_attributes,
            notification_trigger,
            time_interval,
            q,
            geo_q,
            scope_q,
            csf,
            dataset_id,
            temporal_q,
            expand_values,
            json_keys,
            notification,
            throttling,
            expires_at,
            is_active,
            lang,
            jsonld_context,
            local_only,
            split_entities,
            ngsild_conformance,
        } = self;
        subscription_name.is_absent()
            && description.is_absent()
            && entities.is_absent()
            && watched_attributes.is_absent()
            && notification_trigger.is_absent()
            && time_interval.is_absent()
            && q.is_absent()
            && geo_q.is_absent()
            && scope_q.is_absent()
            && csf.is_absent()
            && dataset_id.is_absent()
            && temporal_q.is_absent()
            && expand_values.is_absent()
            && json_keys.is_absent()
            && notification.is_none()
            && throttling.is_absent()
            && expires_at.is_absent()
            && is_active.is_absent()
            && lang.is_absent()
            && jsonld_context.is_absent()
            && local_only.is_absent()
            && split_entities.is_absent()
            && ngsild_conformance.is_absent()
    }
}

#[cfg(test)]
mod tests {
    use crate::{clearable::Clearable, request_body::RequestBody, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
    use serde_json::{Value, json};

    /// One create body naming every member Table 5.2.12-1 defines that a
    /// client may send, in the spelling the specification gives them.
    fn post_body() -> Value {
        json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "subscriptionName": "speeding",
            "description": "vehicles over the limit",
            "entities": [{"type": "Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:.*"}],
            "notificationTrigger": ["entityUpdated"],
            "timeInterval": 30,
            "q": "speed>50",
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [13.4, 52.5], "geoproperty": "location"},
            "scopeQ": "/Madrid",
            "csf": "type==\"ContextSourceRegistration\"",
            "datasetId": ["urn:ngsi-ld:Dataset:1", "@none"],
            "temporalQ": {"timeAt": "2024-01-01T00:00:00Z", "timerel": "after", "timeproperty": "observedAt"},
            "expandValues": "speed",
            "jsonKeys": "payload",
            "notification": {
                "attributes": ["speed"],
                "format": "normalized",
                "endpoint": {"uri": "https://receiver.example.com/notify", "accept": "application/ld+json", "cooldown": 500, "timeout": 3000},
                "sysAttrs": true,
                "showChanges": true,
                "pick": ["id", "owner{name}"],
                "join": "flat",
                "joinLevel": 2
            },
            "expiresAt": "2030-12-31T23:59:59Z",
            "isActive": true,
            "lang": "en",
            "jsonldContext": "https://example.com/context.jsonld",
            "localOnly": true,
            "splitEntities": false,
            "ngsildConformance": "1.4"
        })
    }

    fn decode(body: &Value) -> SubscriptionRequest {
        SubscriptionRequest::from_request_text(body.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a readable subscription body")
    }

    /// Clause 5.8.1: a POST body decoded into the request model and written
    /// back is the body it was, as parsed JSON.
    #[test]
    fn a_subscription_round_trips_through_its_typed_model() {
        let body = post_body();
        let written = serde_json::to_value(decode(&body)).expect("the request writes");
        pretty_assertions::assert_eq!(written, body);
    }

    /// Clause 5.11.2: a Context Source Registration Subscription is the same
    /// data type, carrying the members that select registrations -- here a
    /// `temporalQ` and a `geoQ` -- and no `datasetId`, so its POST body round
    /// trips as an entity subscription's does.
    #[test]
    fn a_context_source_subscription_round_trips_through_its_typed_model() {
        let body = json!({
            "id": "urn:ngsi-ld:Subscription:csr-1",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "watchedAttributes": ["speed"],
            "scopeQ": "/Madrid",
            "geoQ": {"georel": "within", "geometry": "Polygon", "coordinates": [[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 0.0]]]},
            "temporalQ": {"timeAt": "2024-01-01T00:00:00Z", "timerel": "between", "endTimeAt": "2024-02-01T00:00:00Z", "timeproperty": "createdAt"},
            "csf": "tier==\"gold\"",
            "throttling": 2.5,
            "notification": {"attributes": ["speed"], "endpoint": {"uri": "mqtt://broker.example.com:1883/topic"}},
            "isActive": false
        });
        let written = serde_json::to_value(decode(&body)).expect("the request writes");
        pretty_assertions::assert_eq!(written, body);
    }

    /// Clause 5.8.3: a member the fragment leaves out stays out, and an
    /// explicit `null` is told from an absent member for the four members it
    /// clears.
    #[test]
    fn an_update_fragment_tells_an_absent_member_from_a_cleared_one() {
        let absent = decode(&json!({"q": "speed>1"}));
        assert!(absent.geo_q.is_absent() && absent.temporal_q.is_absent() && absent.expand_values.is_absent() && absent.json_keys.is_absent());
        let cleared = decode(&json!({"geoQ": null, "temporalQ": null, "expandValues": null, "jsonKeys": null}));
        assert!(matches!(cleared.geo_q, Clearable::Cleared));
        assert!(matches!(cleared.temporal_q, Clearable::Cleared));
        assert!(matches!(cleared.expand_values, Clearable::Cleared));
        assert!(matches!(cleared.json_keys, Clearable::Cleared));
        pretty_assertions::assert_eq!(
            serde_json::to_value(&cleared).expect("the fragment writes"),
            json!({"geoQ": null, "temporalQ": null, "expandValues": null, "jsonKeys": null})
        );
    }

    /// Clause 5.5.8 with Table 5.2.12-1: a `null` deletes a member of
    /// cardinality `0..1`, and each of them is a member that reads as cleared
    /// and is written back as `null`.
    #[test]
    fn a_null_deletes_every_member_of_cardinality_zero_to_one() {
        for member in [
            "subscriptionName",
            "description",
            "entities",
            "watchedAttributes",
            "notificationTrigger",
            "timeInterval",
            "q",
            "geoQ",
            "scopeQ",
            "csf",
            "datasetId",
            "temporalQ",
            "expandValues",
            "jsonKeys",
            "throttling",
            "expiresAt",
            "isActive",
            "lang",
            "jsonldContext",
            "localOnly",
            "splitEntities",
            "ngsildConformance",
        ] {
            let body = json!({ member: null });
            let request = decode(&body);
            pretty_assertions::assert_eq!(serde_json::to_value(&request).expect("the fragment writes"), body, "{member}");
        }
    }

    /// Clause 5.5.8 and clause 4.5.0: the NGSI-LD Null is `urn:ngsi-ld:null`
    /// where URI strings are valid, and an array of one where an array is
    /// required, so it deletes `jsonldContext`, `datasetId` and
    /// `watchedAttributes` just as `null` does.
    #[test]
    fn the_ngsi_ld_null_deletes_the_members_that_take_uris() {
        let cleared = decode(&json!({"jsonldContext": "urn:ngsi-ld:null", "datasetId": ["urn:ngsi-ld:null"], "watchedAttributes": ["urn:ngsi-ld:null"]}));
        assert!(matches!(cleared.jsonld_context, Clearable::NgsiLdNull));
        assert!(matches!(cleared.dataset_id, Clearable::NgsiLdNull));
        assert!(matches!(cleared.watched_attributes, Clearable::NgsiLdNull));
    }

    /// Clause 5.2.1: "None of the members described admit a null value
    /// directly" outside clauses 5.5.8 and 5.5.12, so a request names the
    /// member it wrote as a null: the JSON `null` of any member, and the URI
    /// Null of a URI member, alone or as the one item of an array. The URI Null
    /// beside another item, or in a text member, is a value.
    #[test]
    fn a_request_names_the_member_it_wrote_a_null_into() {
        for (body, member) in [
            (json!({"jsonldContext": "urn:ngsi-ld:null"}), Some("jsonldContext")),
            (json!({"datasetId": ["urn:ngsi-ld:null"]}), Some("datasetId")),
            (json!({"watchedAttributes": ["urn:ngsi-ld:null"]}), Some("watchedAttributes")),
            (
                json!({"datasetId": ["urn:ngsi-ld:null"], "watchedAttributes": ["urn:ngsi-ld:null"]}),
                Some("watchedAttributes"),
            ),
            (json!({"jsonldContext": null}), Some("jsonldContext")),
            (json!({"ngsildConformance": null}), Some("ngsildConformance")),
            (json!({"q": null, "expiresAt": null}), Some("q")),
            (json!({"datasetId": ["urn:ngsi-ld:null", "@none"]}), None),
            (
                json!({"jsonldContext": "https://example.com/context.jsonld", "description": "urn:ngsi-ld:null"}),
                None,
            ),
            (json!({}), None),
        ] {
            pretty_assertions::assert_eq!(
                decode(&body).null_member().map(|named| named.to_string()),
                member.map(ToOwned::to_owned),
                "{body}"
            );
        }
    }

    /// Clause 5.2.1: every member of cardinality `0..1` written as a JSON
    /// `null` is named, so no member lets a create swallow it.
    #[test]
    fn every_deletable_member_written_as_null_is_named() {
        for member in [
            "subscriptionName",
            "description",
            "entities",
            "watchedAttributes",
            "notificationTrigger",
            "timeInterval",
            "q",
            "geoQ",
            "scopeQ",
            "csf",
            "datasetId",
            "temporalQ",
            "expandValues",
            "jsonKeys",
            "throttling",
            "expiresAt",
            "isActive",
            "lang",
            "jsonldContext",
            "localOnly",
            "splitEntities",
            "ngsildConformance",
        ] {
            pretty_assertions::assert_eq!(
                decode(&json!({ member: null })).null_member().map(|named| named.to_string()),
                Some(member.to_owned())
            );
        }
    }

    /// Clause 4.5.0 names URI strings only: `urn:ngsi-ld:null` is the value of
    /// a text member, and beside another element of an array it is an element.
    #[test]
    fn the_ngsi_ld_null_spelling_is_a_value_where_a_uri_is_not_the_data_type() {
        let request = decode(&json!({"description": "urn:ngsi-ld:null", "datasetId": ["urn:ngsi-ld:null", "@none"]}));
        pretty_assertions::assert_eq!(request.description, Clearable::Set("urn:ngsi-ld:null".to_owned()));
        pretty_assertions::assert_eq!(request.dataset_id.as_set().map(Vec::len), Some(2));
    }

    /// Clause 5.5.4: a `null` for a member that cannot be deleted -- `type` and
    /// `notification` have cardinality `1`, and `id` "cannot be later modified
    /// in update operations" -- is outside its data type, and so is a member of
    /// the wrong type.
    #[test]
    fn a_null_member_that_cannot_be_deleted_and_a_mistyped_member_are_refused() {
        for body in [
            json!({"notification": null}),
            json!({"type": null}),
            json!({"id": null}),
            json!({"ngsildConformance": 1.4}),
            json!({"isActive": "false"}),
            json!({"localOnly": 1}),
            json!({"splitEntities": "no"}),
            json!({"throttling": "slow"}),
            json!({"subscriptionName": 5}),
            json!({"description": ["a"]}),
            json!({"jsonldContext": "not a uri"}),
            json!({"watchedAttributes": "speed"}),
            json!({"type": "Registration"}),
            json!({"expiresAt": "tomorrow"}),
            json!({"id": "not a uri"}),
            json!({"timeInterval": "often"}),
        ] {
            assert!(
                SubscriptionRequest::from_request_text(body.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).is_err(),
                "{body}"
            );
        }
    }

    /// Clause 5.2.12: the members the broker generates are not part of the body
    /// and are ignored.
    #[test]
    fn generated_members_are_ignored() {
        let request = decode(&json!({"status": "paused", "createdAt": "2024-01-01T00:00:00Z", "modifiedAt": "2024-01-01T00:00:00Z"}));
        pretty_assertions::assert_eq!(serde_json::to_value(&request).expect("the request writes"), json!({}));
    }

    /// Clause 6.3.5: an `application/ld+json` body names its context at the top
    /// level, which is not a member of the Subscription.
    #[test]
    fn a_top_level_context_is_not_a_member() {
        let request = SubscriptionRequest::from_request_text(
            br#"{"@context": "https://example.com/ctx.jsonld", "q": "speed>1"}"#,
            CoreTermAliases::keyword_spellings(),
        )
        .expect("the context is skipped");
        assert!(request.q.as_set().is_some());
    }
}
