//! Subscription request validation: the restrictions Table 5.2.12-1 and the
//! clauses around it place on a create body or an update fragment, checked
//! before any of it is expanded or stored.
//!
//! The request model has already read every member as the data type the table
//! gives it, so what is judged here is what a data type cannot say: which
//! members a create requires, which exclude one another, and which values lie
//! outside the range the table allows.

use crate::{
    error::SubscriptionError,
    geo_filter::admit_geo_q,
    notification_validation::{validate_endpoint_present, validate_notification},
    subscription_validation_reason::SubscriptionValidationReason,
};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryCensusLimits;
use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
use andromeda_ngsi_ld_query::{
    filter::{expression::Expr, predicate::Predicate},
    temporal::window::TimeInterval,
};
use chrono::Utc;

fn q_has_comparison(expr: &Expr) -> bool {
    match expr {
        Expr::Pred(pred) => !matches!(pred, Predicate::Exists(_)),
        Expr::And(terms) | Expr::Or(terms) => terms.iter().any(q_has_comparison),
    }
}

/// Validate a subscription create body, admitting its `geoQ` under
/// `geo_limits`.
///
/// # Errors
///
/// Returns the [`SubscriptionError::Validation`] of the first rule the body
/// breaks -- an NGSI-LD Null written into a URI member among them (clause
/// 5.2.1) -- and [`SubscriptionError::GeoQTooLarge`] for a reference geometry
/// past `geo_limits`.
pub fn validate_create_request(request: &SubscriptionRequest, geo_limits: GeometryCensusLimits) -> Result<(), SubscriptionError> {
    // Clause 5.2.1: an NGSI-LD Null is `BadRequestData` outside clauses 5.5.8
    // and 5.5.12, so a create refuses the URI Null; a JSON `null` is dropped by
    // JSON-LD and reads as an absent member.
    if let Some(member) = request.ngsi_ld_null_uri_member() {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::NgsiLdNullOnCreate(member)));
    }
    let Some(notification) = &request.notification else {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::NotificationRequired));
    };
    validate_endpoint_present(notification)?;

    // Per spec 5.8.1.4 (and 5.11.2, which delegates to it for CSR subscriptions),
    // expiresAt must be in the future at creation time.
    validate_expires_at_not_in_past(request)?;

    validate_common_members(request, geo_limits)?;
    validate_subscribed_scope(request)
}

/// Reject a subscription that names nothing to match against (clause 5.2.12).
///
/// Table 5.2.12-1 and the sentence closing clause 5.2.12 place two conditions
/// on the pair `entities` / `watchedAttributes`: "At least one of (a) entities
/// or (b) watchedAttributes shall be present, unless the member localOnly is
/// set to true", and `entities` is "Mandatory if timeInterval is present,
/// unless the execution of the request is limited to local scope (see clause
/// 5.5.13)". `localOnly` is exactly that local scope for a subscription, so
/// both conditions lift together when it is set.
///
/// Checked on creation only. Clause 5.8.2 updates a subscription with a
/// fragment, which need not restate members it is not changing, so the same
/// check against a fragment would refuse a lawful partial update; the
/// subscription an update would store is checked as a whole by
/// [`crate::merged_validation::validate_merged_subscription`].
fn validate_subscribed_scope(request: &SubscriptionRequest) -> Result<(), SubscriptionError> {
    if request.local_only.as_set() == Some(&true) {
        return Ok(());
    }
    // Table 5.2.12-1 forbids an empty `entities` array, so an empty one names no
    // entities and satisfies neither presence rule.
    let names_entities = request.entities.as_set().is_some_and(|entities| !entities.is_empty());
    if !names_entities && request.watched_attributes.as_set().is_none() {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::SelectorRequired));
    }
    if request.time_interval.as_set().is_some() && !names_entities {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::EntitiesRequiredForTimeInterval));
    }
    Ok(())
}

/// Validate that expiresAt, if present, is in the future.
/// Per spec 5.8.2.4, entity subscription updates must reject past expiresAt.
/// CSR subscription updates (5.11.3) do NOT have this restriction. A deleted
/// `expiresAt` names no instant to be past.
///
/// # Errors
///
/// Returns [`SubscriptionValidationReason::ExpiresAtInPast`] for an
/// `expiresAt` that is not after the present instant.
pub fn validate_expires_at_not_in_past(request: &SubscriptionRequest) -> Result<(), SubscriptionError> {
    match request.expires_at.as_set() {
        Some(expires_at) if *expires_at <= Utc::now() => Err(SubscriptionError::Validation(SubscriptionValidationReason::ExpiresAtInPast)),
        Some(_) | None => Ok(()),
    }
}

/// Validate the members shared between create and update, admitting the `geoQ`
/// under `geo_limits`.
///
/// # Errors
///
/// Returns the [`SubscriptionError::Validation`] of the first rule the request
/// breaks, and [`SubscriptionError::GeoQTooLarge`] for a reference geometry
/// past `geo_limits`.
pub fn validate_common_members(request: &SubscriptionRequest, geo_limits: GeometryCensusLimits) -> Result<(), SubscriptionError> {
    if request.time_interval.as_set().is_some() && request.watched_attributes.as_set().is_some() {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalWithWatchedAttributes));
    }

    if request.time_interval.as_set().is_some() && request.throttling.as_set().is_some() {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalWithThrottling));
    }

    // Table 5.2.12-1 restricts `throttling` to "Greater than 0", the same
    // restriction `timeInterval` below carries.
    if request.throttling.as_set().is_some_and(|throttling| *throttling <= 0.0) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::ThrottlingNotPositive));
    }

    if request.time_interval.as_set().is_some_and(|time_interval| *time_interval <= 0.0) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalNotPositive));
    }

    if let Some(q) = request.q.as_set()
        && !q_has_comparison(q)
    {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::QExpressionNoComparison));
    }

    // An update fragment's `null` removes the `geoQ`, which has nothing to admit.
    if let Some(geo_q) = request.geo_q.as_set() {
        admit_geo_q(geo_q, geo_limits)?;
    }

    // Table 5.2.21-1: `endTimeAt` "Cardinality shall be 1 if timerel is equal to
    // between". The same rule builds the window a stored `temporalQ` compiles to,
    // so an admitted `temporalQ` always compiles.
    if let Some(temporal_q) = request.temporal_q.as_set() {
        TimeInterval::from_parts(Some(temporal_q.timerel), Some(temporal_q.time_at), temporal_q.end_time_at)
            .map_err(|error| SubscriptionError::Validation(SubscriptionValidationReason::TemporalQ(error)))?;
    }

    if request.watched_attributes.as_set().is_some_and(Vec::is_empty) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::WatchedAttributesEmpty));
    }

    // Table 5.2.12-1 gives `notificationTrigger` cardinality 0..1 and a
    // default for the absent member; an empty array is neither -- it selects no
    // trigger, so the subscription could never fire -- and is not syntactically
    // correct per the data type definition (clause 5.5.4). This covers create,
    // update and both kinds of subscription, which all validate here.
    if request.notification_trigger.as_set().is_some_and(Vec::is_empty) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::NotificationTriggerEmpty));
    }

    // Table 5.2.12-1, `entities` row: "Empty array (0 length) is not allowed."
    // An empty array would otherwise satisfy both clause 5.2.12 presence rules
    // while selecting nothing.
    if request.entities.as_set().is_some_and(Vec::is_empty) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::EntitiesEmpty));
    }

    if let Some(notification) = &request.notification {
        validate_notification(notification, request.q.as_set())?;
    }

    Ok(())
}

/// Validate the notification an update fragment names: it replaces the stored
/// one (clause 5.8.3), so it has to name the endpoint too.
///
/// # Errors
///
/// Returns [`SubscriptionValidationReason::EndpointRequired`] when the
/// fragment's notification carries no `endpoint`.
pub fn validate_notification_fragment(request: &SubscriptionRequest) -> Result<(), SubscriptionError> {
    request.notification.as_ref().map_or(Ok(()), validate_endpoint_present)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        request_validation::{validate_common_members, validate_create_request, validate_notification_fragment},
        subscription_validation_reason::SubscriptionValidationReason,
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_model::{request_body::RequestBody, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    /// Limits no fixture of this module reaches.
    fn limits() -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::MAX),
            values: JsonValueLimit::new(NonZeroUsize::MAX),
        }
    }

    /// The request body `wire` writes, read as the request model reads it.
    fn request(wire: &Value) -> SubscriptionRequest {
        SubscriptionRequest::from_request_text(wire.to_string().as_bytes()).expect("a readable request")
    }

    #[test]
    fn an_empty_notification_trigger_is_refused() {
        let result = validate_common_members(&request(&json!({"notificationTrigger": []})), limits());
        assert!(
            matches!(
                &result,
                Err(SubscriptionError::Validation(SubscriptionValidationReason::NotificationTriggerEmpty))
            ),
            "got {result:?}"
        );
        assert!(validate_common_members(&request(&json!({"notificationTrigger": ["entityCreated"]})), limits()).is_ok());
    }

    #[test]
    fn create_requires_notification_configuration() {
        let result = validate_create_request(&request(&json!({"type": "Subscription"})), limits());
        assert!(matches!(
            result,
            Err(SubscriptionError::Validation(SubscriptionValidationReason::NotificationRequired))
        ));
    }

    /// Clause 5.2.1: an NGSI-LD Null outside clauses 5.5.8 and 5.5.12 is
    /// `BadRequestData`, so a create refuses `urn:ngsi-ld:null` (alone, or as
    /// the one item of an array) in `jsonldContext`, `datasetId` and
    /// `watchedAttributes`, naming the member; a JSON `null` is dropped by
    /// JSON-LD and stays an absent member, and an update still deletes with it.
    #[test]
    fn a_create_refuses_the_ngsi_ld_null_in_a_uri_member_naming_it() {
        let notification = json!({"endpoint": {"uri": "https://receiver.example.com/notify"}});
        for (member, wire) in [
            ("jsonldContext", json!("urn:ngsi-ld:null")),
            ("datasetId", json!(["urn:ngsi-ld:null"])),
            ("watchedAttributes", json!(["urn:ngsi-ld:null"])),
        ] {
            let body = json!({"type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": notification, member: wire});
            let result = validate_create_request(&request(&body), limits());
            match &result {
                Err(refusal @ SubscriptionError::Validation(SubscriptionValidationReason::NgsiLdNullOnCreate(_))) => {
                    assert!(refusal.to_string().contains(member), "{member}: {refusal}");
                }
                other => panic!("{member}: expected the NGSI-LD Null to be refused, got {other:?}"),
            }
        }
        for member in ["jsonldContext", "datasetId", "watchedAttributes"] {
            let body = json!({"type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": notification, member: null});
            assert!(validate_create_request(&request(&body), limits()).is_ok(), "{member}");
        }
    }

    /// Table 5.2.12-1 types `q` as "a valid query string as per clause 4.9",
    /// whose grammar includes the `LinkedEntityRelation` hop; no subscription
    /// clause restricts it, so a hop is admitted at creation and update alike.
    #[test]
    fn a_subscription_whose_filter_traverses_a_linked_entity_is_accepted() {
        let result = validate_common_members(&request(&json!({"q": "isParkedIn{category}==\"commercial\""})), limits());
        assert!(result.is_ok(), "got {result:?}");
    }

    #[test]
    fn a_traversal_nested_inside_a_boolean_filter_is_accepted_too() {
        let result = validate_common_members(&request(&json!({"q": "speed>50;isParkedIn{category}==\"x\""})), limits());
        assert!(result.is_ok(), "got {result:?}");
    }

    #[test]
    fn a_filter_that_stays_within_one_entity_is_still_accepted() {
        let result = validate_common_members(&request(&json!({"q": "speed>50;brandName==\"Mercedes\""})), limits());
        assert!(result.is_ok(), "got {result:?}");
    }

    /// Clause 5.5.4: a `q` that is not a clause 4.9 expression is data that is
    /// not syntactically correct, refused when the body is read.
    #[test]
    fn a_malformed_q_is_refused_when_the_body_is_read() {
        assert!(SubscriptionRequest::from_request_text(br#"{"q": "speed>"}"#).is_err());
    }

    /// A `q` naming no comparison selects nothing meaningful for one notified
    /// entity.
    #[test]
    fn a_q_without_a_comparison_is_refused() {
        let result = validate_common_members(&request(&json!({"q": "speed"})), limits());
        assert!(
            matches!(
                &result,
                Err(SubscriptionError::Validation(SubscriptionValidationReason::QExpressionNoComparison))
            ),
            "got {result:?}"
        );
    }

    #[test]
    fn a_geo_q_reference_geometry_is_validated_at_create_and_update() {
        // Clause 4.10 cannot evaluate a geometry RFC 7946 gives no reading of,
        // and the request model refuses it when the body is read.
        let error = SubscriptionRequest::from_request_text(br#"{"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}}"#)
            .expect_err("a four-ordinate position is no WGS 84 position");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");

        let elevated = validate_common_members(
            &request(&json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0, 3.0]}})),
            limits(),
        );
        assert!(elevated.is_ok(), "a three-ordinate reference geometry is valid: {elevated:?}");

        // Table 5.2.13-1 gives `geometry` and `coordinates` cardinality 1, and
        // clause 5.8.1.4 refuses a create whose cardinalities are not met.
        let georel_only = SubscriptionRequest::from_request_text(br#"{"geoQ": {"georel": "near;maxDistance==2000"}}"#)
            .expect_err("a geoQ naming only its georel is no GeoQuery");
        assert!(georel_only.to_string().contains("'geometry'"), "got {georel_only}");

        let past_pole =
            SubscriptionRequest::from_request_text(br#"{"geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [-1, 100]}}"#)
                .expect_err("latitude 100 is no WGS 84 position");
        assert!(past_pole.to_string().contains("latitude 100 is outside [-90, 90]"), "got {past_pole}");

        let removed = validate_common_members(&request(&json!({"geoQ": null})), limits());
        assert!(removed.is_ok(), "an update removing the geoQ has nothing to validate: {removed:?}");
    }

    /// Clause 5.2.12: "At least one of (a) entities or (b) watchedAttributes
    /// shall be present, unless the member localOnly is set to true".
    #[test]
    fn a_subscription_naming_neither_entities_nor_watched_attributes_is_rejected() {
        let payload = json!({
            "type": "Subscription",
            "notification": {"endpoint": {"uri": "http://127.0.0.1:8085/notify"}}
        });
        let error = validate_create_request(&request(&payload), limits()).expect_err("clause 5.2.12 requires one of the two");
        assert!(error.to_string().contains("watchedAttributes"), "got {error}");
    }

    /// Either member on its own satisfies clause 5.2.12, and `localOnly` lifts
    /// the requirement altogether (clause 5.5.13).
    #[test]
    fn entities_watched_attributes_or_local_only_each_satisfy_clause_5_2_12() {
        let notification = json!({"endpoint": {"uri": "http://127.0.0.1:8085/notify"}});
        let variants = [
            json!({"type": "Subscription", "notification": notification, "entities": [{"type": "Building"}]}),
            json!({"type": "Subscription", "notification": notification, "watchedAttributes": ["temperature"]}),
            json!({"type": "Subscription", "notification": notification, "localOnly": true}),
        ];
        for payload in variants {
            let result = validate_create_request(&request(&payload), limits());
            assert!(result.is_ok(), "{payload} should be accepted: {result:?}");
        }
    }

    /// Clause 5.2.12, Table 5.2.12-1: `entities` is "Mandatory if timeInterval
    /// is present, unless the execution of the request is limited to local
    /// scope (see clause 5.5.13)".
    #[test]
    fn a_periodic_subscription_without_entities_is_rejected_unless_it_is_local_only() {
        let notification = json!({"endpoint": {"uri": "http://127.0.0.1:8085/notify"}});
        let error = validate_create_request(
            &request(&json!({
                "type": "Subscription",
                "notification": notification,
                "timeInterval": 5,
                "watchedAttributes": ["temperature"]
            })),
            limits(),
        )
        .expect_err("clause 5.2.12 makes entities mandatory beside timeInterval");
        // `watchedAttributes` beside `timeInterval` is refused first by the
        // mutual exclusion Table 5.2.12-1 states for the pair.
        assert!(error.to_string().contains("mutually exclusive"), "got {error}");

        let error = validate_create_request(
            &request(&json!({
                "type": "Subscription",
                "notification": notification,
                "timeInterval": 5,
                "q": "temperature>20"
            })),
            limits(),
        )
        .expect_err("clause 5.2.12 makes entities mandatory beside timeInterval");
        assert!(
            error.to_string().contains("watchedAttributes") || error.to_string().contains("timeInterval"),
            "got {error}"
        );

        let local = validate_create_request(
            &request(&json!({
                "type": "Subscription",
                "notification": notification,
                "timeInterval": 5,
                "localOnly": true
            })),
            limits(),
        );
        assert!(local.is_ok(), "local scope lifts the requirement: {local:?}");
    }

    /// Clause 5.2.14.1: the refusal a create makes of `showChanges` under a
    /// simplified format is made of an update fragment too.
    #[test]
    fn show_changes_under_a_simplified_notification_format_is_rejected_on_create_and_update() {
        for format in ["keyValues", "simplified"] {
            let payload = json!({
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {"format": format, "showChanges": true, "endpoint": {"uri": "http://127.0.0.1:8085/notify"}}
            });
            let error = validate_create_request(&request(&payload), limits()).expect_err("clause 5.2.14.1 forbids the combination");
            assert!(error.to_string().contains("showChanges"), "{format}: {error}");
            assert!(validate_common_members(&request(&payload), limits()).is_err(), "{format}");
        }
    }

    #[test]
    fn time_interval_and_watched_attributes_are_mutually_exclusive() {
        let result = validate_common_members(&request(&json!({"timeInterval": 5, "watchedAttributes": ["temperature"]})), limits());
        assert!(matches!(
            result,
            Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalWithWatchedAttributes))
        ));
    }

    /// Table 5.2.12-1 restricts `throttling` and `timeInterval` to "Greater than
    /// 0".
    #[test]
    fn a_non_positive_throttling_or_time_interval_is_refused() {
        let throttling = validate_common_members(&request(&json!({"throttling": 0})), limits());
        assert!(matches!(
            throttling,
            Err(SubscriptionError::Validation(SubscriptionValidationReason::ThrottlingNotPositive))
        ));
        let interval = validate_common_members(&request(&json!({"timeInterval": -1})), limits());
        assert!(matches!(
            interval,
            Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalNotPositive))
        ));
    }

    /// Clause 5.8.1.4: `expiresAt` must lie in the future at creation.
    #[test]
    fn an_expiration_in_the_past_is_refused_on_create() {
        let payload = json!({
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "expiresAt": "2001-01-01T00:00:00Z",
            "notification": {"endpoint": {"uri": "http://127.0.0.1:8085/notify"}}
        });
        assert!(matches!(
            validate_create_request(&request(&payload), limits()),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::ExpiresAtInPast))
        ));
    }

    /// Table 5.2.21-1: `endTimeAt` "Cardinality shall be 1 if timerel is equal to
    /// between"; the other relations need none.
    #[test]
    fn a_between_temporal_q_without_end_time_at_is_refused_naming_the_member() {
        let error = validate_common_members(
            &request(&json!({"temporalQ": {"timerel": "between", "timeAt": "2020-06-01T00:00:00Z"}})),
            limits(),
        )
        .expect_err("between needs endTimeAt");
        assert!(
            matches!(&error, SubscriptionError::Validation(SubscriptionValidationReason::TemporalQ(_))),
            "got {error:?}"
        );
        assert!(error.to_string().contains("endTimeAt"), "got {error}");
        for accepted in [
            json!({"temporalQ": {"timerel": "between", "timeAt": "2020-06-01T00:00:00Z", "endTimeAt": "2020-07-01T00:00:00Z"}}),
            json!({"temporalQ": {"timerel": "after", "timeAt": "2020-06-01T00:00:00Z"}}),
            json!({"temporalQ": {"timerel": "before", "timeAt": "2020-06-01T00:00:00Z"}}),
            json!({"temporalQ": null}),
        ] {
            let result = validate_common_members(&request(&accepted), limits());
            assert!(result.is_ok(), "{accepted}: {result:?}");
        }
    }

    /// Clause 5.5.4: a body whose member is not the data type Table 5.2.12-1,
    /// 5.2.13-1 or 5.2.21-1 gives it is refused when it is read (answered as
    /// `BadRequestData`). The reader's own message names the member for a
    /// missing or unknown one; for a mistyped value the HTTP extraction layer
    /// retraces the body and names the member, which the integration suite
    /// asserts.
    #[test]
    fn a_member_that_is_not_its_data_type_is_refused_when_the_body_is_read() {
        for (reading, body) in [
            ("missing field `timeAt`", json!({"temporalQ": {"timerel": "after"}})),
            ("missing field `timerel`", json!({"temporalQ": {"timeAt": "2020-06-01T00:00:00Z"}})),
            (
                "unknown field `unknown`",
                json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0], "unknown": 1}}),
            ),
            (
                "expected a string",
                json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0], "geoproperty": 5}}),
            ),
            ("expected a boolean", json!({"isActive": "yes"})),
            ("expected a boolean", json!({"localOnly": "yes"})),
            ("expected a boolean", json!({"splitEntities": 1})),
        ] {
            let error = SubscriptionRequest::from_request_text(body.to_string().as_bytes()).expect_err("a mistyped member is refused");
            assert!(error.to_string().contains(reading), "{body}: got {error}");
        }
    }

    /// A notification an update fragment names replaces the stored one, so it
    /// names its endpoint.
    #[test]
    fn a_notification_fragment_without_an_endpoint_is_refused() {
        let result = validate_notification_fragment(&request(&json!({"notification": {"format": "normalized"}})));
        assert!(
            matches!(result, Err(SubscriptionError::Validation(SubscriptionValidationReason::EndpointRequired))),
            "{result:?}"
        );
        assert!(validate_notification_fragment(&request(&json!({"q": "speed>1"}))).is_ok());
    }
}
