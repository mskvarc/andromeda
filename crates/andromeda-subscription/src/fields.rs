//! Typed subscription persistence values: what a create body or an update
//! fragment becomes once it is merged with what is stored.

use crate::{error::SubscriptionError, store::SubscriptionFields, subscription_validation_reason::SubscriptionValidationReason, variant::DatasetIdPolicy};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::{
    notification::NotificationTrigger,
    notification_request::NotificationRequest,
    query::temporal_query::TemporalQuery,
    subscription::{NotificationParams, Subscription, entity_selector::EntitySelector},
    subscription_request::SubscriptionRequest,
};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, filter::expression::Expr, geo::input::GeoQueryInput, scope::query::ScopeQuery};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use langtag::LangTagBuf;

/// Owned, validated values for one subscription persistence write.
///
/// This shape mirrors the subscription table boundary. Every member is already
/// its domain type; only human-readable text remains textual.
#[derive(Debug)]
pub struct SubscriptionWriteValues {
    /// Optional human-readable subscription name.
    pub subscription_name: Option<String>,
    /// Optional free-text description.
    pub description: Option<String>,
    /// Validated entity selectors.
    pub entities: Option<Vec<EntitySelector>>,
    /// Validated watched attributes.
    pub watched_attributes: Option<Vec<AttributeName>>,
    /// Validated notification triggers.
    pub notification_trigger: Option<Vec<NotificationTrigger>>,
    /// Periodic notification interval in seconds.
    pub time_interval: Option<f64>,
    /// Parsed NGSI-LD query expression.
    pub q: Option<Expr>,
    /// Geo-spatial query.
    pub geo_q: Option<GeoQueryInput>,
    /// Parsed scope query expression.
    pub scope_q: Option<ScopeQuery>,
    /// Parsed context-source filter expression.
    pub csf: Option<Expr>,
    /// Validated `datasetId` selectors, `@none` naming the default instance.
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Whether the subscription is currently active.
    pub is_active: bool,
    /// Validated notification delivery parameters.
    pub notification: Option<NotificationParams>,
    /// Expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Minimum interval between notifications in seconds.
    pub throttling: Option<f64>,
    /// Temporal query.
    pub temporal_q: Option<TemporalQuery>,
    /// Attributes whose `q` operands were coerced through the `@context`
    /// (Table 5.2.12-1 `expandValues`).
    pub expand_values: Option<Vec<AttributeName>>,
    /// Attributes whose `q` object keys were expanded while their values were
    /// left uninterpreted (Table 5.2.12-1 `jsonKeys`).
    pub json_keys: Option<Vec<AttributeName>>,
    /// Validated preferred language.
    pub lang: Option<LangTagBuf>,
    /// Validated JSON-LD context URL.
    pub jsonld_context: Option<IriBuf>,
    /// Whether the subscription applies only to local entities.
    pub local_only: bool,
    /// Whether entities are assumed split across sources; `None` defers to
    /// the deployment default (clause 5.2.12 `splitEntities`).
    pub split_entities: Option<bool>,
    /// The NGSI-LD version a notification's Entities are amended to conform to
    /// (Table 5.2.12-1 `ngsildConformance`); `None` amends nothing.
    pub ngsild_conformance: Option<SpecVersion>,
}

/// The parameters a request's notification describes.
///
/// # Errors
///
/// Returns [`SubscriptionValidationReason::EndpointRequired`] when the
/// notification names no endpoint.
fn notification_params(notification: Option<NotificationRequest>) -> Result<Option<NotificationParams>, SubscriptionError> {
    notification
        .map(NotificationParams::try_from)
        .transpose()
        .map_err(|_| SubscriptionError::Validation(SubscriptionValidationReason::EndpointRequired))
}

impl SubscriptionWriteValues {
    /// The values a create body describes, validated and with its terms
    /// expanded.
    ///
    /// The `jsonldContext` member is taken as it stands: a subscription created
    /// without one has already had the member initialised from the `@context`
    /// applicable at creation (clause 5.8.1.4) by
    /// [`crate::notification_context::initialize_notification_context`], run
    /// before this.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionValidationReason::EndpointRequired`] when the
    /// notification names no endpoint.
    pub fn from_create(request: SubscriptionRequest, dataset_id_policy: DatasetIdPolicy) -> Result<SubscriptionWriteValues, SubscriptionError> {
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
        } = request;
        Ok(SubscriptionWriteValues {
            subscription_name: subscription_name.into_created(),
            description: description.into_created(),
            entities: entities.into_created(),
            watched_attributes: watched_attributes.into_created(),
            notification_trigger: notification_trigger.into_created(),
            time_interval: time_interval.into_created(),
            q: q.into_created(),
            geo_q: geo_q.into_created(),
            scope_q: scope_q.into_created(),
            csf: csf.into_created(),
            dataset_id: match dataset_id_policy {
                DatasetIdPolicy::Retain => dataset_id.into_created(),
                DatasetIdPolicy::Discard => None,
            },
            is_active: is_active.into_created().unwrap_or(true),
            notification: notification_params(notification)?,
            expires_at: expires_at.into_created(),
            throttling: throttling.into_created(),
            temporal_q: temporal_q.into_created(),
            expand_values: expand_values.into_created(),
            json_keys: json_keys.into_created(),
            lang: lang.into_created(),
            jsonld_context: jsonld_context.into_created(),
            local_only: local_only.into_created().unwrap_or(false),
            split_entities: split_entities.into_created(),
            ngsild_conformance: ngsild_conformance.into_created(),
        })
    }

    /// The values an update fragment describes once merged over `existing`
    /// (clause 5.5.8): a member the fragment leaves out keeps its stored value,
    /// a member it sets replaces it, and the NGSI-LD Null deletes it, so a deleted
    /// `isActive` or `localOnly` takes the default Table 5.2.12-1 gives it and
    /// a deleted `expiresAt` leaves a subscription that never expires.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionValidationReason::EndpointRequired`] when the
    /// fragment's notification names no endpoint.
    pub fn merged(
        request: SubscriptionRequest,
        existing: Subscription,
        dataset_id_policy: DatasetIdPolicy,
    ) -> Result<SubscriptionWriteValues, SubscriptionError> {
        let Subscription {
            subscription_name,
            description,
            selection,
            filters,
            notification,
            delivery,
            lifecycle,
            ..
        } = existing;
        let SubscriptionRequest {
            id: _,
            type_: _,
            subscription_name: requested_name,
            description: requested_description,
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
            notification: requested_notification,
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
        } = request;

        Ok(SubscriptionWriteValues {
            subscription_name: requested_name.applied_over(subscription_name),
            description: requested_description.applied_over(description),
            entities: entities.applied_over(selection.entities),
            watched_attributes: watched_attributes.applied_over(selection.watched_attributes),
            notification_trigger: notification_trigger.applied_over(selection.notification_trigger),
            time_interval: time_interval.applied_over(delivery.time_interval),
            q: q.applied_over(filters.q),
            geo_q: geo_q.applied_over(filters.geo_q),
            scope_q: scope_q.applied_over(filters.scope_q),
            csf: csf.applied_over(filters.csf),
            dataset_id: match dataset_id_policy {
                DatasetIdPolicy::Retain => dataset_id.applied_over(filters.dataset_id),
                DatasetIdPolicy::Discard => None,
            },
            is_active: is_active.applied_over(lifecycle.is_active).unwrap_or(true),
            notification: notification_params(requested_notification)?.or(Some(notification)),
            expires_at: expires_at.applied_over(lifecycle.standing.expires_at),
            throttling: throttling.applied_over(delivery.throttling),
            temporal_q: temporal_q.applied_over(filters.temporal_q),
            expand_values: expand_values.applied_over(filters.expand_values),
            json_keys: json_keys.applied_over(filters.json_keys),
            lang: lang.applied_over(delivery.lang),
            jsonld_context: jsonld_context.applied_over(delivery.jsonld_context),
            local_only: local_only.applied_over(delivery.local_only).unwrap_or(false),
            split_entities: split_entities.applied_over(delivery.split_entities),
            ngsild_conformance: ngsild_conformance.applied_over(delivery.ngsild_conformance),
        })
    }

    /// Borrow the values in the shape required by the persistence port.
    pub fn to_fields(&self) -> SubscriptionFields<'_> {
        SubscriptionFields {
            subscription_name: self.subscription_name.as_deref(),
            description: self.description.as_deref(),
            entities: self.entities.as_deref(),
            watched_attributes: self.watched_attributes.as_deref(),
            notification_trigger: self.notification_trigger.as_deref(),
            time_interval: self.time_interval,
            q: self.q.as_ref(),
            geo_q: self.geo_q.as_ref(),
            scope_q: self.scope_q.as_ref(),
            csf: self.csf.as_ref(),
            dataset_id: self.dataset_id.as_deref(),
            is_active: self.is_active,
            notification: self.notification.as_ref(),
            expires_at: self.expires_at,
            throttling: self.throttling,
            temporal_q: self.temporal_q.as_ref(),
            expand_values: self.expand_values.as_deref(),
            json_keys: self.json_keys.as_deref(),
            lang: self.lang.as_ref(),
            jsonld_context: self.jsonld_context.as_ref(),
            local_only: self.local_only,
            split_entities: self.split_entities,
            ngsild_conformance: self.ngsild_conformance,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::SubscriptionError,
        fields::SubscriptionWriteValues,
        merged_validation::validate_merged_subscription,
        subscription_validation_reason::SubscriptionValidationReason,
        variant::DatasetIdPolicy,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, spec_version::SpecVersion};
    use andromeda_ngsi_ld_model::{request_body::RequestBody, subscription::Subscription, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use serde_json::{Value, json};

    /// The request body `wire` writes, read as the request model reads it.
    fn request(wire: &Value) -> SubscriptionRequest {
        SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable request")
    }

    /// The values a create body of `wire` describes, whose notification names
    /// an endpoint.
    fn created(mut wire: Value, policy: DatasetIdPolicy) -> Result<SubscriptionWriteValues, SubscriptionError> {
        if let Some(members) = wire.as_object_mut() {
            members.insert("notification".to_owned(), json!({"endpoint": {"uri": "https://receiver.example.com/notify"}}));
        }
        SubscriptionWriteValues::from_create(request(&wire), policy)
    }

    #[test]
    fn entity_subscription_creation_retains_dataset_identifiers() -> Result<(), SubscriptionError> {
        let values = created(json!({"datasetId": ["urn:ngsi-ld:Dataset:one"]}), DatasetIdPolicy::Retain)?;

        assert_eq!(
            values.dataset_id,
            Some(vec![DatasetSelector::Id("urn:ngsi-ld:Dataset:one".parse().expect("valid dataset URI"))])
        );
        Ok(())
    }

    /// Table 5.2.12-1 types `datasetId` as "Valid URIs, '@none' for including
    /// the default Attribute instances", so `@none` is admitted beside a URI.
    #[test]
    fn entity_subscription_creation_admits_the_none_dataset_selector() -> Result<(), SubscriptionError> {
        let values = created(json!({"datasetId": ["@none", "urn:ngsi-ld:Dataset:one"]}), DatasetIdPolicy::Retain)?;

        assert_eq!(
            values.dataset_id,
            Some(vec![
                DatasetSelector::Default,
                DatasetSelector::Id("urn:ngsi-ld:Dataset:one".parse().expect("valid dataset URI"))
            ])
        );
        Ok(())
    }

    #[test]
    fn a_dataset_selector_that_is_neither_a_uri_nor_none_is_refused_when_the_body_is_read() {
        let refused = SubscriptionRequest::from_request_text(br#"{"datasetId": ["not a uri"]}"#, ContextReading::keyword_spellings());

        assert!(refused.is_err(), "got {refused:?}");
    }

    #[test]
    fn context_source_subscription_creation_discards_dataset_identifiers() -> Result<(), SubscriptionError> {
        let values = created(json!({"datasetId": ["urn:ngsi-ld:Dataset:one"]}), DatasetIdPolicy::Discard)?;

        assert!(values.dataset_id.is_none());
        Ok(())
    }

    /// An entity subscription as it stands in the store, whose opt-in lists
    /// an update fragment is merged over.
    fn stored() -> Subscription {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:stored",
            "type": "Subscription",
            "entities": [{"type": "https://example.com/Vehicle"}],
            "q": "speed>1",
            "expandValues": "status",
            "jsonKeys": "status",
            "geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1, 2]},
            "temporalQ": {"timeAt": "2024-01-01T00:00:00Z", "timerel": "after"},
            "notification": {"endpoint": {"uri": "http://receiver.example.com/notify"}},
            "status": "active",
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-01T00:00:00Z"
        }))
        .expect("the fixture is a valid subscription")
    }

    /// The values `fragment` describes once merged over [`stored`].
    fn merged(fragment: &Value) -> Result<SubscriptionWriteValues, SubscriptionError> {
        SubscriptionWriteValues::merged(request(fragment), stored(), DatasetIdPolicy::Retain)
    }

    /// One merged opt-in list, as wire text.
    fn spelled(names: Option<&[AttributeName]>) -> Vec<&str> {
        names.unwrap_or_default().iter().map(AttributeName::as_str).collect()
    }

    /// Clause 5.8.3 updates a Subscription with "a fragment of the
    /// Subscription", so a member the fragment does not mention is left alone.
    #[test]
    fn an_update_that_names_no_opt_in_leaves_the_stored_lists_alone() -> Result<(), SubscriptionError> {
        let merged = merged(&json!({}))?;

        pretty_assertions::assert_eq!(spelled(merged.expand_values.as_deref()), vec!["status"]);
        pretty_assertions::assert_eq!(spelled(merged.json_keys.as_deref()), vec!["status"]);
        assert!(merged.geo_q.is_some() && merged.temporal_q.is_some());
        Ok(())
    }

    /// The NGSI-LD Null is how a fragment names a member it wants gone, the
    /// same way `geoQ` and `temporalQ` are removed. Without it an opt-in list
    /// could be replaced but never cleared.
    #[test]
    fn the_ngsi_ld_null_clears_a_stored_member() -> Result<(), SubscriptionError> {
        let merged = merged(&json!({"expandValues": "urn:ngsi-ld:null", "geoQ": "urn:ngsi-ld:null", "temporalQ": "urn:ngsi-ld:null"}))?;

        assert!(merged.expand_values.is_none(), "the NGSI-LD Null removes the member");
        assert!(merged.geo_q.is_none() && merged.temporal_q.is_none());
        pretty_assertions::assert_eq!(spelled(merged.json_keys.as_deref()), vec!["status"]);
        Ok(())
    }

    /// The stored subscription of [`stored`] with `members` added to it.
    fn stored_with(members: &Value) -> Subscription {
        let mut wire = serde_json::to_value(stored()).expect("the fixture writes");
        if let (Some(target), Some(extra)) = (wire.as_object_mut(), members.as_object()) {
            target.extend(extra.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        serde_json::from_value(wire).expect("the fixture is a valid subscription")
    }

    /// The values `fragment` describes once merged over `existing`.
    fn merged_over(existing: Subscription, fragment: &Value) -> Result<SubscriptionWriteValues, SubscriptionError> {
        SubscriptionWriteValues::merged(request(fragment), existing, DatasetIdPolicy::Retain)
    }

    /// Clause 5.5.8, which clause 5.8.2.4 applies: "for each member of the
    /// Fragment, whose value is an NGSI-LD Null, contained by the target, the
    /// target member is deleted". `subscriptionName` and `description` have
    /// cardinality `0..1`, and the members the fragment does not name stay.
    #[test]
    fn the_ngsi_ld_null_deletes_the_name_and_the_description() -> Result<(), SubscriptionError> {
        let existing = stored_with(&json!({"subscriptionName": "speeding", "description": "vehicles over the limit"}));

        let merged = merged_over(existing, &json!({"subscriptionName": "urn:ngsi-ld:null"}))?;
        assert!(merged.subscription_name.is_none());
        pretty_assertions::assert_eq!(merged.description.as_deref(), Some("vehicles over the limit"));

        let existing = stored_with(&json!({"subscriptionName": "speeding", "description": "vehicles over the limit"}));
        let merged = merged_over(existing, &json!({"description": "urn:ngsi-ld:null"}))?;
        assert!(merged.description.is_none());
        pretty_assertions::assert_eq!(merged.subscription_name.as_deref(), Some("speeding"));
        Ok(())
    }

    /// Clause 5.5.8: `timeInterval` and `throttling` (cardinality `0..1`) are
    /// deleted by the NGSI-LD Null, and a fragment that leaves them out keeps
    /// them.
    #[test]
    fn the_ngsi_ld_null_deletes_the_time_interval_and_the_throttling() -> Result<(), SubscriptionError> {
        let periodic = || stored_with(&json!({"timeInterval": 30}));
        assert!(merged_over(periodic(), &json!({"timeInterval": "urn:ngsi-ld:null"}))?.time_interval.is_none());
        pretty_assertions::assert_eq!(merged_over(periodic(), &json!({"q": "speed>2"}))?.time_interval, Some(30.0));

        let throttled = || stored_with(&json!({"throttling": 2.5}));
        assert!(merged_over(throttled(), &json!({"throttling": "urn:ngsi-ld:null"}))?.throttling.is_none());
        pretty_assertions::assert_eq!(merged_over(throttled(), &json!({"q": "speed>2"}))?.throttling, Some(2.5));
        Ok(())
    }

    /// Clause 5.5.8 with clause 5.8.2.4: deleting `expiresAt` leaves a
    /// subscription that never expires, which is what the stored write sees; a
    /// fragment that does not name it keeps the expiry.
    #[test]
    fn the_ngsi_ld_null_deletes_the_expiry() -> Result<(), SubscriptionError> {
        let expiring = || stored_with(&json!({"expiresAt": "2099-01-01T00:00:00Z"}));
        assert!(merged_over(expiring(), &json!({"expiresAt": "urn:ngsi-ld:null"}))?.expires_at.is_none());
        assert!(merged_over(expiring(), &json!({"q": "speed>2"}))?.expires_at.is_some());
        Ok(())
    }

    /// Clause 5.5.8 with Table 5.2.12-1: a deleted `isActive` is the default
    /// `true`, and a deleted `localOnly` is `false`.
    #[test]
    fn the_ngsi_ld_null_returns_is_active_and_local_only_to_their_defaults() -> Result<(), SubscriptionError> {
        let existing = || stored_with(&json!({"isActive": false, "localOnly": true}));
        let kept = merged_over(existing(), &json!({"q": "speed>2"}))?;
        assert!(!kept.is_active && kept.local_only);

        let deleted = merged_over(existing(), &json!({"isActive": "urn:ngsi-ld:null", "localOnly": "urn:ngsi-ld:null"}))?;
        assert!(deleted.is_active && !deleted.local_only);
        Ok(())
    }

    /// Clause 5.5.8: `splitEntities`, `lang`, `jsonldContext` and
    /// `ngsildConformance` (cardinality `0..1`) are deleted by the NGSI-LD Null
    /// of clause 4.5.0, whatever their data type.
    #[test]
    fn the_ngsi_ld_null_deletes_the_delivery_members() -> Result<(), SubscriptionError> {
        let existing = || {
            stored_with(&json!({
                "splitEntities": true,
                "lang": "en",
                "jsonldContext": "https://example.com/context.jsonld",
                "ngsildConformance": "1.4"
            }))
        };
        let kept = merged_over(existing(), &json!({"q": "speed>2"}))?;
        assert!(kept.split_entities.is_some() && kept.lang.is_some() && kept.jsonld_context.is_some() && kept.ngsild_conformance.is_some());

        for member in ["splitEntities", "lang", "jsonldContext", "ngsildConformance"] {
            let deleted = merged_over(existing(), &json!({ member: "urn:ngsi-ld:null" }))?;
            let remaining = [
                deleted.split_entities.is_some(),
                deleted.lang.is_some(),
                deleted.jsonld_context.is_some(),
                deleted.ngsild_conformance.is_some(),
            ];
            let expected = ["splitEntities", "lang", "jsonldContext", "ngsildConformance"].map(|other| other != member);
            pretty_assertions::assert_eq!(remaining, expected, "{member}");
        }

        Ok(())
    }

    /// Clause 5.5.8: the filters `q`, `scopeQ`, `csf` and `datasetId` (cardinality
    /// `0..1`) are deleted by the NGSI-LD Null, `datasetId` also by an array of
    /// one NGSI-LD Null (clause 4.5.0), and one the fragment leaves out stays.
    #[test]
    fn the_ngsi_ld_null_deletes_the_filters() -> Result<(), SubscriptionError> {
        let existing = || stored_with(&json!({"scopeQ": "/Madrid", "csf": "tier==\"gold\"", "datasetId": ["urn:ngsi-ld:Dataset:one"]}));
        let kept = merged_over(existing(), &json!({"description": "d"}))?;
        assert!(kept.q.is_some() && kept.scope_q.is_some() && kept.csf.is_some() && kept.dataset_id.is_some());

        let deleted = merged_over(
            existing(),
            &json!({"q": "urn:ngsi-ld:null", "scopeQ": "urn:ngsi-ld:null", "csf": "urn:ngsi-ld:null", "datasetId": "urn:ngsi-ld:null"}),
        )?;
        assert!(deleted.q.is_none() && deleted.scope_q.is_none() && deleted.csf.is_none() && deleted.dataset_id.is_none());

        let deleted = merged_over(existing(), &json!({"datasetId": ["urn:ngsi-ld:null"]}))?;
        assert!(deleted.dataset_id.is_none() && deleted.q.is_some());
        Ok(())
    }

    /// Clause 5.5.8 with Table 5.2.12-1: `entities` and `watchedAttributes` have
    /// cardinality `0..1`, so the NGSI-LD Null deletes either, but the subscription
    /// that remains needs one of them unless it is `localOnly` (clause 5.2.12),
    /// so deleting the only selector is refused once merged.
    #[test]
    fn the_ngsi_ld_null_deletes_a_selector_only_while_another_remains() -> Result<(), SubscriptionError> {
        let only_entities = merged_over(stored(), &json!({"entities": "urn:ngsi-ld:null"}))?;
        assert!(only_entities.entities.is_none());
        assert!(matches!(
            validate_merged_subscription(&only_entities),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::SelectorRequired))
        ));

        let both = || stored_with(&json!({"watchedAttributes": ["https://example.com/speed"]}));
        let without_entities = merged_over(both(), &json!({"entities": "urn:ngsi-ld:null"}))?;
        assert!(without_entities.entities.is_none() && without_entities.watched_attributes.is_some());
        validate_merged_subscription(&without_entities)?;

        let without_watched = merged_over(both(), &json!({"watchedAttributes": "urn:ngsi-ld:null"}))?;
        assert!(without_watched.watched_attributes.is_none() && without_watched.entities.is_some());
        validate_merged_subscription(&without_watched)?;

        let local = merged_over(stored_with(&json!({"localOnly": true})), &json!({"entities": "urn:ngsi-ld:null"}))?;
        validate_merged_subscription(&local)
    }

    /// Clause 5.5.8: `notificationTrigger` has cardinality `0..1` and a
    /// default, so the NGSI-LD Null deletes it.
    #[test]
    fn the_ngsi_ld_null_deletes_the_notification_trigger() -> Result<(), SubscriptionError> {
        let existing = stored_with(&json!({"notificationTrigger": ["entityCreated"]}));
        assert!(
            merged_over(existing, &json!({"notificationTrigger": "urn:ngsi-ld:null"}))?
                .notification_trigger
                .is_none()
        );
        Ok(())
    }

    /// Table 5.2.12-1 gives `notification` cardinality `1`, so it cannot be
    /// deleted: the NGSI-LD Null for it or for the `type` is refused when the
    /// body is read (clause 5.5.4), and so is a JSON `null` for them or for
    /// the `id` (clause 5.2.1), so the stored notification is never dropped.
    #[test]
    fn a_null_for_a_member_that_cannot_be_deleted_is_refused() {
        for body in [
            json!({"notification": null}),
            json!({"type": null}),
            json!({"id": null}),
            json!({"notification": "urn:ngsi-ld:null"}),
            json!({"type": "urn:ngsi-ld:null"}),
        ] {
            assert!(
                SubscriptionRequest::from_request_text(body.to_string().as_bytes(), ContextReading::keyword_spellings()).is_err(),
                "{body}"
            );
        }
    }

    /// Anything else replaces the stored list, once the comma-separated form
    /// Table 5.2.12-1 defines has been validated.
    #[test]
    fn a_named_list_replaces_the_stored_one() -> Result<(), SubscriptionError> {
        let merged = merged(&json!({"jsonKeys": "payload,extra"}))?;

        pretty_assertions::assert_eq!(spelled(merged.json_keys.as_deref()), vec!["payload", "extra"]);
        Ok(())
    }

    /// Clause 5.8.3: a notification the fragment names replaces the stored one
    /// whole, and one it leaves out is kept.
    #[test]
    fn a_named_notification_replaces_the_stored_one_and_an_absent_one_is_kept() -> Result<(), SubscriptionError> {
        let kept = merged(&json!({"q": "speed>2"}))?;
        pretty_assertions::assert_eq!(
            kept.notification.map(|params| params.endpoint.uri.to_string()),
            Some("http://receiver.example.com/notify".to_owned())
        );

        let replaced = merged(&json!({"notification": {"endpoint": {"uri": "http://other.example.com/notify"}}}))?;
        pretty_assertions::assert_eq!(
            replaced.notification.map(|params| params.endpoint.uri.to_string()),
            Some("http://other.example.com/notify".to_owned())
        );
        Ok(())
    }

    #[test]
    fn a_creation_naming_no_expiration_has_none() -> Result<(), SubscriptionError> {
        let values = created(json!({}), DatasetIdPolicy::Retain)?;

        assert!(values.expires_at.is_none() && values.is_active && !values.local_only);
        Ok(())
    }

    /// Table 5.2.12-1 gives `ngsildConformance` the form `major.minor`, so a
    /// well-formed one is carried through to the write values.
    #[test]
    fn a_well_formed_ngsild_conformance_is_parsed_into_a_spec_version() -> Result<(), SubscriptionError> {
        let values = created(json!({"ngsildConformance": "1.4"}), DatasetIdPolicy::Retain)?;

        pretty_assertions::assert_eq!(values.ngsild_conformance, Some(SpecVersion::V1_4));
        Ok(())
    }

    /// A version the fallback tables of clause 4.3.6.8 do not name is still of
    /// the form the table defines, so it is admitted; clause 4.3.6.8
    /// introduces each fallback at a version and applies it to everything
    /// older, which is what decides what `1.5` receives.
    #[test]
    fn a_version_the_fallback_tables_do_not_name_is_still_admitted() -> Result<(), SubscriptionError> {
        let values = created(json!({"ngsildConformance": "1.5"}), DatasetIdPolicy::Retain)?;

        pretty_assertions::assert_eq!(values.ngsild_conformance, Some(SpecVersion::new(1, 5)));
        Ok(())
    }

    /// Anything outside the `major.minor` form Table 5.2.12-1 defines is
    /// syntactically incorrect according to the API data type definition, and
    /// clause 5.5.4 makes that `BadRequestData` rather than a dropped member;
    /// the request model refuses it when the body is read. The NGSI-LD Null is
    /// not among them: clause 5.5.8 makes it the deletion of the member, which
    /// [`the_ngsi_ld_null_deletes_the_delivery_members`] pins.
    #[test]
    fn a_malformed_ngsild_conformance_is_refused_when_the_body_is_read() {
        for token in [json!("1"), json!("1.2.3"), json!("abc"), json!(""), json!("v1.4"), json!(1.4), json!(null)] {
            let body = json!({"ngsildConformance": token});
            assert!(
                SubscriptionRequest::from_request_text(body.to_string().as_bytes(), ContextReading::keyword_spellings()).is_err(),
                "{token} must be refused"
            );
        }
    }

    /// An absent member amends nothing, and there is no default version to
    /// fall back to.
    #[test]
    fn an_absent_ngsild_conformance_leaves_no_version() -> Result<(), SubscriptionError> {
        let values = created(json!({}), DatasetIdPolicy::Retain)?;

        assert!(values.ngsild_conformance.is_none());
        Ok(())
    }

    /// A create naming no endpoint is no set of notification parameters.
    #[test]
    fn a_creation_whose_notification_names_no_endpoint_is_refused() {
        let refused = SubscriptionWriteValues::from_create(request(&json!({"notification": {"format": "normalized"}})), DatasetIdPolicy::Retain);

        assert!(matches!(refused, Err(SubscriptionError::Validation(_))), "{refused:?}");
    }
}
