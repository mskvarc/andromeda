//! Subscription payload validation: the restrictions Table 5.2.12-1 and the
//! clauses around it place on a create body or an update fragment, checked
//! before any of it is expanded or stored.

use crate::error::SubscriptionError;
use andromeda_ngsi_ld_geometry::{geometry::Geometry, geometry_type::GeometryType};
use andromeda_ngsi_ld_query::{
    filter::{expression::Expr, parser, predicate::Predicate},
    projection::exclusivity::{Presence, ProjectionSelectors, check_projection_exclusivity},
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use chrono::Utc;
use serde_json::Value;

fn q_has_comparison(expr: &Expr) -> bool {
    match expr {
        Expr::Pred(pred) => !matches!(pred, Predicate::Exists(_)),
        Expr::And(terms) | Expr::Or(terms) => terms.iter().any(q_has_comparison),
    }
}

/// Validate the common parts of a subscription create payload.
pub fn validate_create_payload(payload: &Value) -> Result<(), SubscriptionError> {
    if payload.get("notification").is_none() {
        return Err(SubscriptionError::validation("notification is required"));
    }

    if let Some(notification) = payload.get("notification") {
        if notification.get("endpoint").is_none() {
            return Err(SubscriptionError::validation("notification.endpoint is required"));
        }
        if let Some(endpoint) = notification.get("endpoint")
            && endpoint.get("uri").is_none()
        {
            return Err(SubscriptionError::validation("notification.endpoint.uri is required"));
        }
    }

    // Per spec 5.8.1.4 (and 5.11.2, which delegates to it for CSR subscriptions),
    // expiresAt must be in the future at creation time.
    if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str()) {
        // Format validation is handled by validate_common_fields; only check future here.
        if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val)
            && expires_at <= Utc::now()
        {
            return Err(SubscriptionError::validation("expiresAt must be in the future"));
        }
    }

    validate_common_fields(payload)?;
    validate_subscribed_scope(payload)?;
    Ok(())
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
/// check against a fragment would refuse a lawful partial update.
fn validate_subscribed_scope(payload: &Value) -> Result<(), SubscriptionError> {
    if payload.get("localOnly").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    // Table 5.2.12-1 forbids an empty `entities` array, so an empty one names no
    // entities and satisfies neither presence rule.
    let names_entities = payload
        .get("entities")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|entities| !entities.is_empty());
    if !names_entities && payload.get("watchedAttributes").is_none() {
        return Err(SubscriptionError::validation(
            "at least one of 'entities' or 'watchedAttributes' is required, unless 'localOnly' is true",
        ));
    }
    if payload.get("timeInterval").is_some() && !names_entities {
        return Err(SubscriptionError::validation(
            "'entities' is required when 'timeInterval' is present, unless the request is limited to local scope",
        ));
    }
    Ok(())
}

/// Validate that expiresAt, if present, is in the future.
/// Per spec 5.8.2.4, entity subscription updates must reject past expiresAt.
/// CSR subscription updates (5.11.3) do NOT have this restriction.
pub fn validate_expires_at_not_in_past(payload: &Value) -> Result<(), SubscriptionError> {
    if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str())
        && let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val)
        && expires_at <= Utc::now()
    {
        return Err(SubscriptionError::validation("expiresAt must be in the future"));
    }
    Ok(())
}

/// Validate fields shared between create and update.
pub fn validate_common_fields(payload: &Value) -> Result<(), SubscriptionError> {
    if let Some(type_val) = payload.get("type")
        && type_val.as_str() != Some("Subscription")
    {
        return Err(SubscriptionError::validation("type must be 'Subscription'"));
    }

    // expiresAt format validation only. The future check is applied separately by each
    // caller (create per 5.8.1.4; entity-subscription update per 5.8.2.4), not here.
    if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str())
        && chrono::DateTime::parse_from_rfc3339(expires_at_val).is_err()
    {
        return Err(SubscriptionError::validation("expiresAt must be a valid ISO 8601 date-time"));
    }

    if payload.get("timeInterval").is_some() && payload.get("watchedAttributes").is_some() {
        return Err(SubscriptionError::validation("timeInterval and watchedAttributes are mutually exclusive"));
    }

    if payload.get("timeInterval").is_some() && payload.get("throttling").is_some() {
        return Err(SubscriptionError::validation("timeInterval and throttling are mutually exclusive"));
    }

    // Table 5.2.12-1 restricts `throttling` to "Greater than 0", the same
    // restriction `timeInterval` below carries.
    if let Some(throttling) = payload.get("throttling").and_then(serde_json::Value::as_f64)
        && throttling <= 0.0
    {
        return Err(SubscriptionError::validation("throttling must be greater than 0"));
    }

    if let Some(time_interval) = payload.get("timeInterval").and_then(serde_json::Value::as_f64)
        && time_interval <= 0.0
    {
        return Err(SubscriptionError::validation("timeInterval must be greater than 0"));
    }

    if let Some(q_str) = payload.get("q").and_then(|v| v.as_str()) {
        let expr = parser::parse(q_str).map_err(|e| SubscriptionError::validation(format!("Invalid q expression: {e}")))?;
        if !q_has_comparison(&expr) {
            return Err(SubscriptionError::validation(
                "Invalid q expression: must contain at least one comparison operator",
            ));
        }
        // Clause 4.9's `{}` axis addresses an attribute of a *linked* entity,
        // and clause 5.8.6 evaluates a subscription's filter against the entity
        // data carried by the change that triggered it. That data holds the
        // Relationship's object URI, not the entity it names, so a traversal
        // resolves nothing and the subscription could never fire. Accepting one
        // would be worse than rejecting it: the client would see a live
        // subscription that silently never notifies. Lifting this needs
        // store-backed matching, which resolves the linked entity at match time
        // the way the query path already does.
        if expr.contains_linked_hop() {
            return Err(SubscriptionError::validation(
                "Invalid q expression: a filter that traverses into a linked entity is not supported for subscriptions, \
                 because a subscription is matched against the notified entity's own data",
            ));
        }
    }

    if let Some(geo_q) = payload.get("geoQ") {
        validate_geo_q_geometry(geo_q)?;
    }

    if let Some(watched) = payload.get("watchedAttributes")
        && let Some(arr) = watched.as_array()
        && arr.is_empty()
    {
        return Err(SubscriptionError::validation("watchedAttributes must not be empty"));
    }

    // Table 5.2.12-1, `entities` row: "Empty array (0 length) is not allowed."
    // An empty array would otherwise satisfy both clause 5.2.12 presence rules
    // while selecting nothing.
    if let Some(entities) = payload.get("entities")
        && let Some(arr) = entities.as_array()
        && arr.is_empty()
    {
        return Err(SubscriptionError::validation("entities must not be empty"));
    }

    if let Some(notification) = payload.get("notification") {
        validate_show_changes_format(notification)?;
        validate_notification_projection(notification)?;
    }

    Ok(())
}

/// Reject a notification combining clause 4.21 selectors that shape the same
/// projection.
///
/// Table 5.2.14.1-1 gives `NotificationParams` the same three members a query
/// carries on its URL -- `pick`, `omit` and the deprecated `attributes`, "a
/// synonym for pick" -- so it carries the same rule about combining them. The
/// rule itself is owned by
/// [`andromeda_ngsi_ld_query::projection::exclusivity`], which the query
/// bindings apply to their own parameters.
fn validate_notification_projection(notification: &Value) -> Result<(), SubscriptionError> {
    check_projection_exclusivity(ProjectionSelectors {
        pick: Presence::of(notification.get("pick")),
        omit: Presence::of(notification.get("omit")),
        attrs: Presence::of(notification.get("attributes")),
    })
    .map_err(|conflict| SubscriptionError::validation(conflict.to_string()))
}

/// Reject a notification asking for `showChanges` under a simplified format
/// (clause 5.2.14.1).
///
/// Table 5.2.14.1-1 states that "showChanges cannot be true in case format is
/// `keyValues`". The simplified rendering of clause 4.5.4 reduces an attribute
/// to its value alone, leaving nowhere for the `previousValue`,
/// `previousObject` or `previousLanguageMap` that `showChanges` exists to
/// deliver, so the combination asks for something no notification could carry.
/// Refusing it at creation and update is what stops a client holding a live
/// subscription whose defining option silently does nothing.
///
/// `simplified` is the same rendering under the name Table 5.2.14.1-1 gives as
/// the primary one, with `keyValues` listed as its synonym, so both are
/// refused. A `format` outside the four of Table 5.2.14.1-1 is a different
/// failure and is left to the caller that reads it.
fn validate_show_changes_format(notification: &Value) -> Result<(), SubscriptionError> {
    let Some(true) = notification.get("showChanges").and_then(Value::as_bool) else {
        return Ok(());
    };
    let simplified = notification
        .get("format")
        .and_then(Value::as_str)
        .and_then(|format| format.parse::<ResponseFormat>().ok())
        .is_some_and(ResponseFormat::is_simplified);
    if simplified {
        return Err(SubscriptionError::validation(
            "notification.showChanges cannot be true when notification.format is keyValues",
        ));
    }
    Ok(())
}

/// Reject a `geoQ` whose `geometry` and `coordinates` do not form a usable
/// RFC 7946 geometry (clause 4.10, Table 5.2.13-1).
///
/// Checked here rather than left to compilation because a stored `geoQ` that
/// will not decode is dropped silently when the subscription is compiled: the
/// client would hold a live subscription that never notifies and never says
/// why. A position with an ordinate count RFC 7946 section 3.1.1 gives no
/// meaning to is a `BadRequestData` at the boundary instead.
///
/// A `geoQ` carrying neither member is left alone; the two are optional on the
/// wire and their absence is a different failure from a geometry that will not
/// decode.
fn validate_geo_q_geometry(geo_q: &Value) -> Result<(), SubscriptionError> {
    let (Some(geometry_type), Some(coordinates)) = (geo_q.get("geometry"), geo_q.get("coordinates")) else {
        return Ok(());
    };
    let geometry_type = geometry_type
        .as_str()
        .ok_or_else(|| SubscriptionError::validation("geoQ geometry must be a GeoJSON geometry type string"))?
        .parse::<GeometryType>()
        .map_err(|error| SubscriptionError::validation(format!("geoQ geometry: {error}")))?;
    let coordinates = match coordinates {
        Value::String(raw) => serde_json::from_str::<Value>(raw).map_err(|error| SubscriptionError::validation(format!("geoQ coordinates: {error}")))?,
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => other.clone(), // clone: the GeoJSON decoder consumes the value it decodes
    };
    Geometry::from_type_and_coordinates(geometry_type, coordinates)
        .map(|_geometry| ())
        .map_err(|error| SubscriptionError::validation(format!("geoQ reference geometry: {error}")))
}

/// Validate the notification fragment in an update payload.
pub fn validate_notification_fragment(payload: &Value) -> Result<(), SubscriptionError> {
    if let Some(notification_val) = payload.get("notification") {
        match notification_val {
            Value::Null => {
                return Err(SubscriptionError::validation("notification cannot be set to null"));
            }
            Value::Object(_) => {
                if let Some(endpoint_val) = notification_val.get("endpoint") {
                    match endpoint_val {
                        Value::Null => {
                            return Err(SubscriptionError::validation("notification.endpoint cannot be set to null"));
                        }
                        Value::Object(_) => {
                            if let Some(uri_val) = endpoint_val.get("uri") {
                                match uri_val {
                                    Value::Null => {
                                        return Err(SubscriptionError::validation("notification.endpoint.uri cannot be set to null"));
                                    }
                                    Value::String(s) if s.trim().is_empty() => {
                                        return Err(SubscriptionError::validation("notification.endpoint.uri must be a non-empty string"));
                                    }
                                    Value::String(_) | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {}
                                }
                            }
                        }
                        _other @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => {}
                    }
                } else {
                    return Err(SubscriptionError::validation("notification.endpoint is required"));
                }
            }
            _other @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => {
                return Err(SubscriptionError::validation("notification must be an object"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        validation::{validate_common_fields, validate_create_payload},
    };
    use serde_json::json;

    #[test]
    fn create_requires_notification_configuration() {
        let result = validate_create_payload(&serde_json::json!({"type": "Subscription"}));
        assert!(matches!(result, Err(SubscriptionError::Validation { detail }) if detail == "notification is required"));
    }

    /// Clause 4.9 defines the `{}` axis over an entity's linked entities, and
    /// clause 5.8.6 matches a subscription against the notification's entity
    /// data alone. The broker has no store-backed matcher, so a subscription
    /// carrying a traversal filter would be accepted and could never fire.
    /// Rejecting it at validation is the honest answer, and it covers updates as
    /// well because both paths validate here.
    #[test]
    fn a_subscription_whose_filter_traverses_a_linked_entity_is_rejected() {
        let result = validate_common_fields(&serde_json::json!({"q": "isParkedIn{category==\"commercial\"}"}));
        assert!(
            matches!(&result, Err(SubscriptionError::Validation { detail }) if detail.contains("linked entity")),
            "got {result:?}"
        );
    }

    #[test]
    fn a_traversal_nested_inside_a_boolean_filter_is_rejected_too() {
        let result = validate_common_fields(&serde_json::json!({"q": "speed>50;isParkedIn{category==\"x\"}"}));
        assert!(matches!(result, Err(SubscriptionError::Validation { .. })), "got {result:?}");
    }

    #[test]
    fn a_filter_that_stays_within_one_entity_is_still_accepted() {
        let result = validate_common_fields(&serde_json::json!({"q": "speed>50;brandName==\"Mercedes\""}));
        assert!(result.is_ok(), "got {result:?}");
    }

    #[test]
    fn a_geo_q_reference_geometry_is_validated_at_create_and_update() {
        let result = validate_common_fields(&serde_json::json!({
            "geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}
        }));
        let error = result.expect_err("clause 4.10 cannot evaluate a geometry RFC 7946 gives no reading of");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");

        let elevated = validate_common_fields(&serde_json::json!({
            "geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0, 3.0]}
        }));
        assert!(elevated.is_ok(), "a three-ordinate reference geometry is valid: {elevated:?}");

        let georel_only = validate_common_fields(&serde_json::json!({"geoQ": {"georel": "near;maxDistance==2000"}}));
        assert!(georel_only.is_ok(), "an incomplete geoQ is a different failure: {georel_only:?}");
    }

    /// Clause 5.2.12: "At least one of (a) entities or (b) watchedAttributes
    /// shall be present, unless the member localOnly is set to true".
    #[test]
    fn a_subscription_naming_neither_entities_nor_watched_attributes_is_rejected() {
        let payload = json!({
            "type": "Subscription",
            "notification": {"endpoint": {"uri": "http://127.0.0.1:8085/notify"}}
        });
        let error = validate_create_payload(&payload).expect_err("clause 5.2.12 requires one of the two");
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
            let result = validate_create_payload(&payload);
            assert!(result.is_ok(), "{payload} should be accepted: {result:?}");
        }
    }

    /// Clause 5.2.12, Table 5.2.12-1: `entities` is "Mandatory if timeInterval
    /// is present, unless the execution of the request is limited to local
    /// scope (see clause 5.5.13)".
    #[test]
    fn a_periodic_subscription_without_entities_is_rejected_unless_it_is_local_only() {
        let notification = json!({"endpoint": {"uri": "http://127.0.0.1:8085/notify"}});
        let error = validate_create_payload(&json!({
            "type": "Subscription",
            "notification": notification,
            "timeInterval": 5,
            "watchedAttributes": ["temperature"]
        }))
        .expect_err("clause 5.2.12 makes entities mandatory beside timeInterval");
        // `watchedAttributes` beside `timeInterval` is refused first by the
        // mutual exclusion Table 5.2.12-1 states for the pair.
        assert!(error.to_string().contains("mutually exclusive"), "got {error}");

        let error = validate_create_payload(&json!({
            "type": "Subscription",
            "notification": notification,
            "timeInterval": 5,
            "q": "temperature>20"
        }))
        .expect_err("clause 5.2.12 makes entities mandatory beside timeInterval");
        assert!(
            error.to_string().contains("watchedAttributes") || error.to_string().contains("timeInterval"),
            "got {error}"
        );

        let local = validate_create_payload(&json!({
            "type": "Subscription",
            "notification": notification,
            "timeInterval": 5,
            "localOnly": true
        }));
        assert!(local.is_ok(), "local scope lifts the requirement: {local:?}");
    }

    /// Clause 5.2.14.1, Table 5.2.14.1-1: "showChanges cannot be true in case
    /// format is `keyValues`". The simplified rendering of clause 4.5.4 has
    /// nowhere to carry a `previousValue`, so the combination is refused rather
    /// than silently doing nothing.
    #[test]
    fn show_changes_under_a_simplified_notification_format_is_rejected() {
        for format in ["keyValues", "simplified"] {
            let payload = json!({
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {"format": format, "showChanges": true, "endpoint": {"uri": "http://127.0.0.1:8085/notify"}}
            });
            let error = validate_create_payload(&payload).expect_err("clause 5.2.14.1 forbids the combination");
            assert!(error.to_string().contains("showChanges"), "{format}: {error}");
            // The same fragment is refused on update, which validates the
            // notification it carries the same way.
            assert!(validate_common_fields(&payload).is_err(), "{format}");
        }
    }

    /// The combination is only refused for the simplified renderings; the
    /// normalized and concise ones carry the previous member fine.
    #[test]
    fn show_changes_under_a_normalized_or_concise_format_is_accepted() {
        for format in ["normalized", "concise"] {
            let payload = json!({
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {"format": format, "showChanges": true, "endpoint": {"uri": "http://127.0.0.1:8085/notify"}}
            });
            let result = validate_create_payload(&payload);
            assert!(result.is_ok(), "{format}: {result:?}");
        }
    }

    #[test]
    fn time_interval_and_watched_attributes_are_mutually_exclusive() {
        let result = validate_common_fields(&serde_json::json!({"timeInterval": 5, "watchedAttributes": ["temperature"]}));
        assert!(matches!(result, Err(SubscriptionError::Validation { detail }) if detail == "timeInterval and watchedAttributes are mutually exclusive"));
    }
}
