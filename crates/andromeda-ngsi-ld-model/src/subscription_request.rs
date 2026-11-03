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
//! member from a deleted one: the NGSI-LD Null `urn:ngsi-ld:null` deletes it,
//! whatever its data type, and so does an array holding the Null alone for a
//! member that takes an array (clause 4.5.0). A JSON `null` is no value of any
//! member (clause 5.2.1) and is refused as the body is read. The members of
//! cardinality `1` (`type` and `notification`) and `id`, which "cannot be later
//! modified in update operations", cannot be deleted, so the Null for one of
//! them is a value outside its data type (clause 5.5.4). The members the broker
//! generates (`status`, `createdAt`, `modifiedAt`) are not part of the body and
//! are ignored when a client sends them, whatever they hold.
//!
//! Tables 5.2.12-1 and 5.2.12-2 define the Subscription in full and give it no
//! place to keep a member they do not name, so a body naming one is refused
//! rather than stored without it. That is Andromeda's own bound, stricter than
//! the clause: it stops a misspelt `watchedAttributes` from becoming a
//! subscription that watches every attribute. A user `@context` term that
//! aliases a member is that member, as JSON-LD expansion requires.

use crate::{
    attribute_name_csv,
    clearable::Clearable,
    clearable_array,
    clearable_parsed,
    clearable_timestamp,
    delivery_seconds,
    ignored_member,
    notification::NotificationTrigger,
    notification_request::NotificationRequest,
    query::temporal_query::TemporalQuery,
    request_body::RequestBody,
    subscription::{SubscriptionType, entity_selector::EntitySelector},
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(field_defaults(default = Default::default()))]
pub struct SubscriptionRequest {
    /// Subscription identifier (clause 5.2.12: a URI, not necessarily an
    /// NGSI-LD URN); the broker mints one when a create names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Identifier>,
    /// JSON-LD type, which can only be `Subscription`. `None` when the body
    /// leaves it out: a create refuses that (Table 5.2.12-1 gives it
    /// cardinality 1), an update fragment may do it.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<SubscriptionType>,
    /// Human-readable subscription name.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub subscription_name: Clearable<String>,
    /// Human-readable description of the subscription purpose.
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub description: Clearable<String>,
    /// Entity selectors defining which entities trigger notifications.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "clearable_array::deserialize",
        serialize_with = "clearable_array::serialize"
    )]
    pub entities: Clearable<Vec<EntitySelector>>,
    /// Attribute names to watch for changes.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "clearable_array::deserialize",
        serialize_with = "clearable_array::serialize"
    )]
    pub watched_attributes: Clearable<Vec<AttributeName>>,
    /// Conditions that trigger a notification.
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "clearable_array::deserialize",
        serialize_with = "clearable_array::serialize"
    )]
    pub notification_trigger: Clearable<Vec<NotificationTrigger>>,
    /// Periodic notification interval in seconds.
    #[serde(default, skip_serializing_if = "Clearable::is_absent", serialize_with = "delivery_seconds::clearable::serialize")]
    pub time_interval: Clearable<f64>,
    /// NGSI-LD query expression to filter matching entities (clause 4.9).
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub q: Clearable<Expr>,
    /// Geo-spatial query expression (clause 5.2.13).
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
        deserialize_with = "clearable_array::deserialize",
        serialize_with = "clearable_array::serialize"
    )]
    pub dataset_id: Clearable<Vec<DatasetSelector>>,
    /// Temporal query expression (clause 5.2.21).
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
    pub temporal_q: Clearable<TemporalQuery>,
    /// Attributes whose `q` values are expanded by JSON-LD type coercion
    /// (Table 5.2.12-1 `expandValues`).
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "attribute_name_csv::clearable::deserialize",
        serialize_with = "attribute_name_csv::clearable::serialize"
    )]
    pub expand_values: Clearable<Vec<AttributeName>>,
    /// Attributes whose values are uninterpretable as JSON-LD (Table 5.2.12-1
    /// `jsonKeys`).
    #[serde(
        default,
        skip_serializing_if = "Clearable::is_absent",
        deserialize_with = "attribute_name_csv::clearable::deserialize",
        serialize_with = "attribute_name_csv::clearable::serialize"
    )]
    pub json_keys: Clearable<Vec<AttributeName>>,
    /// Notification delivery configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Clearable::is_absent")]
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
    /// The generated `status` (Table 5.2.12-2): accepted and ignored.
    #[serde(default, skip_serializing, deserialize_with = "ignored_member::deserialize")]
    #[builder(setter(skip))]
    pub status: (),
    /// The generated `createdAt` (clause 5.2.2): accepted and ignored.
    #[serde(default, skip_serializing, deserialize_with = "ignored_member::deserialize")]
    #[builder(setter(skip))]
    pub created_at: (),
    /// The generated `modifiedAt` (clause 5.2.2): accepted and ignored.
    #[serde(default, skip_serializing, deserialize_with = "ignored_member::deserialize")]
    #[builder(setter(skip))]
    pub modified_at: (),
}

impl RequestBody for SubscriptionRequest {}

impl SubscriptionRequest {
    /// Every member, in the order of Table 5.2.12-1, which the body wrote as
    /// the NGSI-LD Null (clause 4.5.0).
    ///
    /// Clause 5.5.8 gives the Null the meaning "the target member is deleted"
    /// in an update; a create has no member to delete, and clause 5.2.1 makes
    /// an NGSI-LD Null elsewhere `BadRequestData`.
    pub fn deleted_members(&self) -> impl Iterator<Item = SubscriptionMember> {
        [
            (SubscriptionMember::SubscriptionName, self.subscription_name.is_ngsi_ld_null()),
            (SubscriptionMember::Description, self.description.is_ngsi_ld_null()),
            (SubscriptionMember::Entities, self.entities.is_ngsi_ld_null()),
            (SubscriptionMember::WatchedAttributes, self.watched_attributes.is_ngsi_ld_null()),
            (SubscriptionMember::NotificationTrigger, self.notification_trigger.is_ngsi_ld_null()),
            (SubscriptionMember::TimeInterval, self.time_interval.is_ngsi_ld_null()),
            (SubscriptionMember::Q, self.q.is_ngsi_ld_null()),
            (SubscriptionMember::GeoQ, self.geo_q.is_ngsi_ld_null()),
            (SubscriptionMember::ScopeQ, self.scope_q.is_ngsi_ld_null()),
            (SubscriptionMember::Csf, self.csf.is_ngsi_ld_null()),
            (SubscriptionMember::DatasetId, self.dataset_id.is_ngsi_ld_null()),
            (SubscriptionMember::TemporalQ, self.temporal_q.is_ngsi_ld_null()),
            (SubscriptionMember::ExpandValues, self.expand_values.is_ngsi_ld_null()),
            (SubscriptionMember::JsonKeys, self.json_keys.is_ngsi_ld_null()),
            (SubscriptionMember::Throttling, self.throttling.is_ngsi_ld_null()),
            (SubscriptionMember::ExpiresAt, self.expires_at.is_ngsi_ld_null()),
            (SubscriptionMember::IsActive, self.is_active.is_ngsi_ld_null()),
            (SubscriptionMember::Lang, self.lang.is_ngsi_ld_null()),
            (SubscriptionMember::JsonldContext, self.jsonld_context.is_ngsi_ld_null()),
            (SubscriptionMember::LocalOnly, self.local_only.is_ngsi_ld_null()),
            (SubscriptionMember::SplitEntities, self.split_entities.is_ngsi_ld_null()),
            (SubscriptionMember::NgsildConformance, self.ngsild_conformance.is_ngsi_ld_null()),
        ]
        .into_iter()
        .filter_map(|(member, deleted)| deleted.then_some(member))
    }

    /// The first member [`SubscriptionRequest::deleted_members`] names.
    #[must_use]
    pub fn ngsi_ld_null_member(&self) -> Option<SubscriptionMember> {
        self.deleted_members().next()
    }

    /// The body with every member it deletes left out, as a payload written
    /// for a version that reads no NGSI-LD Null leaves the deletion out
    /// (clause 4.3.6.8).
    #[must_use]
    pub fn without_deletions(self) -> SubscriptionRequest {
        SubscriptionRequest {
            subscription_name: self.subscription_name.without_deletion(),
            description: self.description.without_deletion(),
            entities: self.entities.without_deletion(),
            watched_attributes: self.watched_attributes.without_deletion(),
            notification_trigger: self.notification_trigger.without_deletion(),
            time_interval: self.time_interval.without_deletion(),
            q: self.q.without_deletion(),
            geo_q: self.geo_q.without_deletion(),
            scope_q: self.scope_q.without_deletion(),
            csf: self.csf.without_deletion(),
            dataset_id: self.dataset_id.without_deletion(),
            temporal_q: self.temporal_q.without_deletion(),
            expand_values: self.expand_values.without_deletion(),
            json_keys: self.json_keys.without_deletion(),
            throttling: self.throttling.without_deletion(),
            expires_at: self.expires_at.without_deletion(),
            is_active: self.is_active.without_deletion(),
            lang: self.lang.without_deletion(),
            jsonld_context: self.jsonld_context.without_deletion(),
            local_only: self.local_only.without_deletion(),
            split_entities: self.split_entities.without_deletion(),
            ngsild_conformance: self.ngsild_conformance.without_deletion(),
            ..self
        }
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
            status: (),
            created_at: (),
            modified_at: (),
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{request_body::RequestBody, subscription_member::SubscriptionMember, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
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
        SubscriptionRequest::from_request_text(body.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable subscription body")
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
            "geoQ": {"georel": "within", "geometry": "Polygon", "coordinates": [[[0, 0], [10, 0], [10, 10], [0, 0]]]},
            "temporalQ": {"timeAt": "2024-01-01T00:00:00Z", "timerel": "between", "endTimeAt": "2024-02-01T00:00:00Z", "timeproperty": "createdAt"},
            "csf": "tier==\"gold\"",
            "throttling": 2.5,
            "notification": {"attributes": ["speed"], "endpoint": {"uri": "mqtt://broker.example.com:1883/topic"}},
            "isActive": false
        });
        let written = serde_json::to_value(decode(&body)).expect("the request writes");
        pretty_assertions::assert_eq!(written, body);
    }

    /// Every member Table 5.2.12-1 gives cardinality `0..1`, beside whether
    /// its data type is an array.
    const DELETABLE_MEMBERS: [(&str, bool); 22] = [
        ("subscriptionName", false),
        ("description", false),
        ("entities", true),
        ("watchedAttributes", true),
        ("notificationTrigger", true),
        ("timeInterval", false),
        ("q", false),
        ("geoQ", false),
        ("scopeQ", false),
        ("csf", false),
        ("datasetId", true),
        ("temporalQ", false),
        ("expandValues", false),
        ("jsonKeys", false),
        ("throttling", false),
        ("expiresAt", false),
        ("isActive", false),
        ("lang", false),
        ("jsonldContext", false),
        ("localOnly", false),
        ("splitEntities", false),
        ("ngsildConformance", false),
    ];

    fn refused(body: &Value) -> bool {
        SubscriptionRequest::from_request_text(body.to_string().as_bytes(), ContextReading::keyword_spellings()).is_err()
    }

    /// Clause 5.5.8: a member the fragment leaves out stays out, and the
    /// NGSI-LD Null is told from an absent member.
    #[test]
    fn an_update_fragment_tells_an_absent_member_from_a_deleted_one() {
        let absent = decode(&json!({"q": "speed>1"}));
        assert!(absent.geo_q.is_absent() && absent.temporal_q.is_absent() && absent.expand_values.is_absent() && absent.json_keys.is_absent());
        let deleted =
            decode(&json!({"geoQ": "urn:ngsi-ld:null", "temporalQ": "urn:ngsi-ld:null", "expandValues": "urn:ngsi-ld:null", "jsonKeys": "urn:ngsi-ld:null"}));
        assert!(deleted.geo_q.is_ngsi_ld_null() && deleted.temporal_q.is_ngsi_ld_null());
        assert!(deleted.expand_values.is_ngsi_ld_null() && deleted.json_keys.is_ngsi_ld_null());
    }

    /// Clauses 4.5.0 and 5.5.8 with Table 5.2.12-1: the NGSI-LD Null deletes
    /// every member of cardinality `0..1`, a number, a boolean and an object
    /// member included, and each is written back as the Null, an array member
    /// in the array form.
    #[test]
    fn the_ngsi_ld_null_deletes_every_member_of_cardinality_zero_to_one() {
        for (member, array) in DELETABLE_MEMBERS {
            let request = decode(&json!({ member: "urn:ngsi-ld:null" }));
            pretty_assertions::assert_eq!(request.ngsi_ld_null_member().map(|named| named.to_string()), Some(member.to_owned()));
            let written = if array { json!(["urn:ngsi-ld:null"]) } else { json!("urn:ngsi-ld:null") };
            pretty_assertions::assert_eq!(
                serde_json::to_value(&request).expect("the fragment writes"),
                json!({ member: written }),
                "{member}"
            );
        }
    }

    /// Clause 4.5.0: where an array is required, an array with one single
    /// NGSI-LD Null deletes the member as the bare Null does.
    #[test]
    fn an_array_of_one_ngsi_ld_null_deletes_an_array_member() {
        for (member, _) in DELETABLE_MEMBERS.into_iter().filter(|(_, array)| *array) {
            pretty_assertions::assert_eq!(
                decode(&json!({ member: ["urn:ngsi-ld:null"] }))
                    .ngsi_ld_null_member()
                    .map(|named| named.to_string()),
                Some(member.to_owned())
            );
        }
    }

    /// Clause 5.2.1: the first member written as the NGSI-LD Null is named,
    /// and the Null beside another element of an array is an element.
    #[test]
    fn a_request_names_the_first_member_it_wrote_the_ngsi_ld_null_into() {
        for (body, member) in [
            (
                json!({"datasetId": ["urn:ngsi-ld:null"], "watchedAttributes": ["urn:ngsi-ld:null"]}),
                Some("watchedAttributes"),
            ),
            (json!({"q": "urn:ngsi-ld:null", "expiresAt": "urn:ngsi-ld:null"}), Some("q")),
            (
                json!({"jsonldContext": "https://example.com/context.jsonld", "description": "urn:ngsi-ld:null"}),
                Some("description"),
            ),
            (json!({"datasetId": ["urn:ngsi-ld:null", "@none"]}), None),
            (json!({}), None),
        ] {
            pretty_assertions::assert_eq!(
                decode(&body).ngsi_ld_null_member().map(|named| named.to_string()),
                member.map(ToOwned::to_owned),
                "{body}"
            );
        }
        let elements = decode(&json!({"datasetId": ["urn:ngsi-ld:null", "@none"]}));
        pretty_assertions::assert_eq!(elements.dataset_id.as_set().map(Vec::len), Some(2));
    }

    /// Clause 5.2.1: no member admits a JSON `null`, whether it may be deleted
    /// or not, at the top level or nested.
    #[test]
    fn a_json_null_is_refused_on_every_member() {
        for (member, _) in DELETABLE_MEMBERS {
            assert!(refused(&json!({ member: null })), "{member}");
        }
        for body in [
            json!({"id": null}),
            json!({"type": null}),
            json!({"notification": null}),
            json!({"notification": {"endpoint": {"uri": "https://receiver.example.com/notify"}, "format": null}}),
            json!({"entities": [{"type": "Vehicle", "idPattern": null}]}),
            json!({"watchedAttributes": ["speed", null]}),
            json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1, 2], "geoproperty": null}}),
        ] {
            assert!(refused(&body), "{body}");
        }
    }

    /// Clause 5.5.4: the NGSI-LD Null for a member that cannot be deleted --
    /// `type` and `notification` have cardinality `1`, and `id` "cannot be
    /// later modified in update operations" -- is outside its data type, and
    /// so is a member of the wrong type.
    #[test]
    fn the_ngsi_ld_null_on_a_member_that_cannot_be_deleted_and_a_mistyped_member_are_refused() {
        for body in [
            json!({"notification": "urn:ngsi-ld:null"}),
            json!({"type": "urn:ngsi-ld:null"}),
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
            assert!(refused(&body), "{body}");
        }
    }

    /// Clause 5.2.12: the members the broker generates are not part of the body
    /// and are ignored.
    #[test]
    fn generated_members_are_ignored() {
        let request = decode(&json!({"status": "paused", "createdAt": "2024-01-01T00:00:00Z", "modifiedAt": "2024-01-01T00:00:00Z"}));
        pretty_assertions::assert_eq!(serde_json::to_value(&request).expect("the request writes"), json!({}));
    }

    /// Table 5.2.12-1 defines the Subscription in full: a misspelt member and
    /// an extension member are refused naming the member.
    #[test]
    fn an_unknown_member_is_refused_naming_it() {
        for member in ["watchedAtributes", "priority"] {
            let body = json!({ member: ["speed"] });
            let error = SubscriptionRequest::from_request_text(body.to_string().as_bytes(), ContextReading::keyword_spellings())
                .expect_err("an unknown member is refused");
            assert!(error.to_string().contains(&format!("unknown field `{member}`")), "{member}: got {error}");
        }
    }

    /// Table 5.2.12-2 and clause 5.2.2: the generated members are accepted
    /// whatever they hold, and never written back.
    #[test]
    fn the_generated_members_are_accepted_whatever_they_hold() {
        let request = decode(&json!({"status": 5, "createdAt": null, "modifiedAt": [true]}));
        pretty_assertions::assert_eq!(serde_json::to_value(&request).expect("the request writes"), json!({}));
    }

    /// Clause 6.3.5: an `application/ld+json` body names its context at the top
    /// level, which is not a member of the Subscription.
    #[test]
    fn a_top_level_context_is_not_a_member() {
        let request = SubscriptionRequest::from_request_text(
            br#"{"@context": "https://example.com/ctx.jsonld", "q": "speed>1"}"#,
            ContextReading::keyword_spellings(),
        )
        .expect("the context is skipped");
        assert!(request.q.as_set().is_some());
    }

    /// Every deleted member is named, in the order of Table 5.2.12-1, and
    /// leaving the deletions out keeps every member the fragment sets.
    #[test]
    fn the_deletions_are_named_and_can_be_left_out() {
        let fragment: SubscriptionRequest = serde_json::from_value(json!({
            "expiresAt": "urn:ngsi-ld:null",
            "entities": ["urn:ngsi-ld:null"],
            "q": "speed>1",
            "throttling": "urn:ngsi-ld:null"
        }))
        .expect("a fragment");
        pretty_assertions::assert_eq!(
            fragment.deleted_members().collect::<Vec<_>>(),
            [SubscriptionMember::Entities, SubscriptionMember::Throttling, SubscriptionMember::ExpiresAt]
        );
        let kept = fragment.without_deletions();
        pretty_assertions::assert_eq!(kept.deleted_members().count(), 0);
        pretty_assertions::assert_eq!(serde_json::to_value(&kept).expect("the fragment writes"), json!({"q": "speed>1"}));
    }
}
