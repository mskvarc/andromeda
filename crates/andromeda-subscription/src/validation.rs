//! Subscription payload validation.

use crate::error::SubscriptionError;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_query::{
    filter::{expansion::expand_q_expr, expression::Expr, parser, predicate::Predicate, serialize},
    type_selection::{expansion::expand_type_expression, grammar::TypeExprRaw},
};
use chrono::Utc;
use serde_json::Value;
use tracing::warn;

fn q_has_comparison(expr: &Expr) -> bool {
    match expr {
        Expr::Pred(pred) => !matches!(pred, Predicate::Exists(_)),
        Expr::And(terms) | Expr::Or(terms) => terms.iter().any(q_has_comparison),
    }
}

// ---------------------------------------------------------------------------
// Payload validation (shared by create + update)
// ---------------------------------------------------------------------------

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

    // Per spec 5.11.2 / 5.8.2, expiresAt must be in the future at creation time.
    if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str()) {
        // Format validation is handled by validate_common_fields; only check future here.
        if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val)
            && expires_at <= Utc::now()
        {
            return Err(SubscriptionError::validation("expiresAt must be in the future"));
        }
    }

    validate_common_fields(payload)?;
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

    // expiresAt format validation (but not future check — that's create-only per spec 5.11.2)
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

    if let Some(throttling) = payload.get("throttling").and_then(|v| v.as_f64())
        && throttling < 0.0
    {
        return Err(SubscriptionError::validation("throttling must be greater than or equal to 0"));
    }

    if let Some(time_interval) = payload.get("timeInterval").and_then(|v| v.as_f64())
        && time_interval <= 0.0
    {
        return Err(SubscriptionError::validation("timeInterval must be greater than 0"));
    }

    if let Some(q_str) = payload.get("q").and_then(|v| v.as_str()) {
        let expr = parser::parse(q_str).map_err(|e| SubscriptionError::validation(format!("Invalid q expression: {}", e)))?;
        if !q_has_comparison(&expr) {
            return Err(SubscriptionError::validation(
                "Invalid q expression: must contain at least one comparison operator",
            ));
        }
        // Clause 4.9's `{}` axis addresses an attribute of a *linked* entity,
        // and clause 5.8.1 evaluates a subscription's filter against the entity
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

    if let Some(watched) = payload.get("watchedAttributes")
        && let Some(arr) = watched.as_array()
        && arr.is_empty()
    {
        return Err(SubscriptionError::validation("watchedAttributes must not be empty"));
    }

    Ok(())
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
                                    _other => {}
                                }
                            }
                        }
                        _other => {}
                    }
                } else {
                    return Err(SubscriptionError::validation("notification.endpoint is required"));
                }
            }
            _other => {
                return Err(SubscriptionError::validation("notification must be an object"));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Term expansion (shared by create + update)
// ---------------------------------------------------------------------------

/// Expand entities, watchedAttributes, notification.attributes, q, and geoQ
/// in a subscription payload. Context hosting is NOT handled here — the
/// entity-subscription service adds that on top.
pub async fn expand_common_subscription_terms(engine: &JsonLdEngine, payload: &mut Value, context: &ContextSpec) -> Result<(), SubscriptionError> {
    let prepared_context = engine.prepare_term_context(context);
    let expander = ContextualExpander::new(engine, &prepared_context);
    // Expand entities[].type
    if let Some(entities) = payload.get_mut("entities")
        && let Some(arr) = entities.as_array_mut()
    {
        for entity_sel in arr.iter_mut() {
            if let Some(type_val) = entity_sel.get("type").and_then(|v| v.as_str()) {
                let raw = type_val.parse::<TypeExprRaw>()?;
                match expand_type_expression(&raw, &expander).await.and_then(|expanded| expanded.into_raw()) {
                    Ok(expanded) => {
                        if let Some(obj) = entity_sel.as_object_mut() {
                            obj.insert("type".to_string(), Value::String(expanded.to_string()));
                        }
                    }
                    Err(error) => return Err(SubscriptionError::from(error)),
                }
            }
        }
    }

    // Expand watchedAttributes
    if let Some(watched) = payload.get_mut("watchedAttributes")
        && let Some(arr) = watched.as_array_mut()
    {
        for attr in arr.iter_mut() {
            if let Some(term) = attr.as_str() {
                match engine.expand_term(term, &prepared_context).await {
                    Ok(expanded) => {
                        *attr = Value::String(expanded.to_string());
                    }
                    Err(e) => {
                        if e.is_context_not_available() {
                            return Err(SubscriptionError::from(e));
                        }
                        warn!("Failed to expand watchedAttribute '{}': {}", term, e);
                    }
                }
            }
        }
    }

    // Expand notification.attributes
    if let Some(notification) = payload.get_mut("notification")
        && let Some(attrs) = notification.get_mut("attributes")
        && let Some(arr) = attrs.as_array_mut()
    {
        for attr in arr.iter_mut() {
            if let Some(term) = attr.as_str() {
                match engine.expand_term(term, &prepared_context).await {
                    Ok(expanded) => {
                        *attr = Value::String(expanded.to_string());
                    }
                    Err(e) => {
                        if e.is_context_not_available() {
                            return Err(SubscriptionError::from(e));
                        }
                        warn!("Failed to expand notification attribute '{}': {}", term, e);
                    }
                }
            }
        }
    }

    // Expand q expression attribute terms
    if let Some(q_str) = payload.get("q").and_then(|v| v.as_str()).map(String::from)
        && let Ok(expr) = parser::parse(&q_str)
        && let Ok(expanded) = expand_q_expr(expr, &expander).await
    {
        let expanded_str = serialize::serialize(&expanded);
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("q".to_string(), Value::String(expanded_str));
        }
    }

    // Expand geoQ.geoproperty
    if let Some(geo_q) = payload.get_mut("geoQ")
        && let Some(geoprop) = geo_q.get("geoproperty").and_then(|v| v.as_str()).map(String::from)
    {
        match engine.expand_term(&geoprop, &prepared_context).await {
            Ok(expanded) => {
                if let Some(obj) = geo_q.as_object_mut() {
                    obj.insert("geoproperty".to_string(), Value::String(expanded.to_string()));
                }
            }
            Err(e) => {
                if e.is_context_not_available() {
                    return Err(SubscriptionError::from(e));
                }
                warn!("Failed to expand geoQ.geoproperty '{}': {}", geoprop, e);
            }
        }
    }

    // Strip @context — it's been used for expansion
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("@context");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        validation::{validate_common_fields, validate_create_payload},
    };

    #[test]
    fn create_requires_notification_configuration() {
        let result = validate_create_payload(&serde_json::json!({"type": "Subscription"}));
        assert!(matches!(result, Err(SubscriptionError::Validation { detail }) if detail == "notification is required"));
    }

    /// Clause 4.9 defines the `{}` axis over an entity's linked entities, and
    /// clause 5.8.1 matches a subscription against the notification's entity
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
    fn time_interval_and_watched_attributes_are_mutually_exclusive() {
        let result = validate_common_fields(&serde_json::json!({"timeInterval": 5, "watchedAttributes": ["temperature"]}));
        assert!(matches!(result, Err(SubscriptionError::Validation { detail }) if detail == "timeInterval and watchedAttributes are mutually exclusive"));
    }
}

// ---------------------------------------------------------------------------
// Response serialization (shared by get / list)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Response fixup helpers (shared by entity + CSR subscription serialization)
// ---------------------------------------------------------------------------
