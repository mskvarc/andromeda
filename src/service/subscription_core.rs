//! Shared subscription CRUD logic used by both entity subscriptions and
//! context-source subscriptions.

use crate::{
    core::error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine, term_expansion::ContextualExpander},
    repository::subscription_store::params::SubscriptionFields,
};
use andromeda_domain::subscription::Subscription;
use andromeda_ngsi_ld_query::filter::{expansion::expand_q_expr, expression::Expr, parser, predicate::Predicate, serialize};
use chrono::Utc;
use serde_json::Value;
use tracing::warn;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a q expression contains at least one comparison (not just existence checks).
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
pub fn validate_create_payload(payload: &Value) -> Result<(), AndromedaError> {
    if payload.get("notification").is_none() {
        return Err(AndromedaError::subscription_validation("notification is required"));
    }

    if let Some(notification) = payload.get("notification") {
        if notification.get("endpoint").is_none() {
            return Err(AndromedaError::subscription_validation("notification.endpoint is required"));
        }
        if let Some(endpoint) = notification.get("endpoint")
            && endpoint.get("uri").is_none()
        {
            return Err(AndromedaError::subscription_validation("notification.endpoint.uri is required"));
        }
    }

    // Per spec 5.11.2 / 5.8.2, expiresAt must be in the future at creation time.
    if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str()) {
        // Format validation is handled by validate_common_fields; only check future here.
        if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val)
            && expires_at <= Utc::now()
        {
            return Err(AndromedaError::subscription_validation("expiresAt must be in the future"));
        }
    }

    validate_common_fields(payload)?;
    Ok(())
}

/// Validate that expiresAt, if present, is in the future.
/// Per spec 5.8.2.4, entity subscription updates must reject past expiresAt.
/// CSR subscription updates (5.11.3) do NOT have this restriction.
pub fn validate_expires_at_not_in_past(payload: &Value) -> Result<(), AndromedaError> {
    if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str())
        && let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val)
        && expires_at <= Utc::now()
    {
        return Err(AndromedaError::subscription_validation("expiresAt must be in the future"));
    }
    Ok(())
}

/// Validate fields shared between create and update.
pub fn validate_common_fields(payload: &Value) -> Result<(), AndromedaError> {
    if let Some(type_val) = payload.get("type")
        && type_val.as_str() != Some("Subscription")
    {
        return Err(AndromedaError::subscription_validation("type must be 'Subscription'"));
    }

    // expiresAt format validation (but not future check — that's create-only per spec 5.11.2)
    if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str())
        && chrono::DateTime::parse_from_rfc3339(expires_at_val).is_err()
    {
        return Err(AndromedaError::subscription_validation("expiresAt must be a valid ISO 8601 date-time"));
    }

    if payload.get("timeInterval").is_some() && payload.get("watchedAttributes").is_some() {
        return Err(AndromedaError::subscription_validation(
            "timeInterval and watchedAttributes are mutually exclusive",
        ));
    }

    if payload.get("timeInterval").is_some() && payload.get("throttling").is_some() {
        return Err(AndromedaError::subscription_validation("timeInterval and throttling are mutually exclusive"));
    }

    if let Some(throttling) = payload.get("throttling").and_then(|v| v.as_f64())
        && throttling < 0.0
    {
        return Err(AndromedaError::subscription_validation("throttling must be greater than or equal to 0"));
    }

    if let Some(time_interval) = payload.get("timeInterval").and_then(|v| v.as_f64())
        && time_interval <= 0.0
    {
        return Err(AndromedaError::subscription_validation("timeInterval must be greater than 0"));
    }

    if let Some(q_str) = payload.get("q").and_then(|v| v.as_str()) {
        let expr = parser::parse(q_str).map_err(|e| AndromedaError::subscription_validation(format!("Invalid q expression: {}", e)))?;
        if !q_has_comparison(&expr) {
            return Err(AndromedaError::subscription_validation(
                "Invalid q expression: must contain at least one comparison operator",
            ));
        }
    }

    if let Some(watched) = payload.get("watchedAttributes")
        && let Some(arr) = watched.as_array()
        && arr.is_empty()
    {
        return Err(AndromedaError::subscription_validation("watchedAttributes must not be empty"));
    }

    Ok(())
}

/// Validate the notification fragment in an update payload.
pub fn validate_notification_fragment(payload: &Value) -> Result<(), AndromedaError> {
    if let Some(notification_val) = payload.get("notification") {
        match notification_val {
            Value::Null => {
                return Err(AndromedaError::subscription_validation("notification cannot be set to null"));
            }
            Value::Object(_) => {
                if let Some(endpoint_val) = notification_val.get("endpoint") {
                    match endpoint_val {
                        Value::Null => {
                            return Err(AndromedaError::subscription_validation("notification.endpoint cannot be set to null"));
                        }
                        Value::Object(_) => {
                            if let Some(uri_val) = endpoint_val.get("uri") {
                                match uri_val {
                                    Value::Null => {
                                        return Err(AndromedaError::subscription_validation("notification.endpoint.uri cannot be set to null"));
                                    }
                                    Value::String(s) if s.trim().is_empty() => {
                                        return Err(AndromedaError::subscription_validation("notification.endpoint.uri must be a non-empty string"));
                                    }
                                    _other => {}
                                }
                            }
                        }
                        _other => {}
                    }
                } else {
                    return Err(AndromedaError::subscription_validation("notification.endpoint is required"));
                }
            }
            _other => {
                return Err(AndromedaError::subscription_validation("notification must be an object"));
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
pub async fn expand_common_subscription_terms(engine: &JsonLdEngine, payload: &mut Value, context: &ContextSpec) -> Result<(), AndromedaError> {
    // Expand entities[].type
    if let Some(entities) = payload.get_mut("entities")
        && let Some(arr) = entities.as_array_mut()
    {
        for entity_sel in arr.iter_mut() {
            if let Some(type_val) = entity_sel.get("type").and_then(|v| v.as_str()) {
                // Detect regex alternation patterns and skip expansion
                if type_val.contains('|') || (type_val.contains('(') && type_val.contains(')')) {
                    continue;
                }
                match engine.expand_type(type_val, context).await {
                    Ok(expanded) => {
                        if let Some(obj) = entity_sel.as_object_mut() {
                            obj.insert("type".to_string(), Value::String(expanded));
                        }
                    }
                    Err(e) => {
                        if e.is_context_not_available() {
                            return Err(AndromedaError::from(e));
                        }
                        warn!("Failed to expand entity selector type '{}': {}", type_val, e);
                    }
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
                match engine.expand_term(term, context).await {
                    Ok(expanded) => {
                        *attr = Value::String(expanded);
                    }
                    Err(e) => {
                        if e.is_context_not_available() {
                            return Err(AndromedaError::from(e));
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
                match engine.expand_term(term, context).await {
                    Ok(expanded) => {
                        *attr = Value::String(expanded);
                    }
                    Err(e) => {
                        if e.is_context_not_available() {
                            return Err(AndromedaError::from(e));
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
        && let Ok(expanded) = expand_q_expr(expr, &ContextualExpander::new(engine, context)).await
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
        match engine.expand_term(&geoprop, context).await {
            Ok(expanded) => {
                if let Some(obj) = geo_q.as_object_mut() {
                    obj.insert("geoproperty".to_string(), Value::String(expanded));
                }
            }
            Err(e) => {
                if e.is_context_not_available() {
                    return Err(AndromedaError::from(e));
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

// ---------------------------------------------------------------------------
// Response serialization (shared by get / list)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Response fixup helpers (shared by entity + CSR subscription serialization)
// ---------------------------------------------------------------------------

/// Convert whole-number floats to integers for timeInterval and throttling fields.
/// Spec requires these as integers but serde may serialize as float.
pub fn fixup_integer_fields(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        for key in &["timeInterval", "throttling"] {
            if let Some(val) = obj.get(*key).and_then(|v| v.as_f64())
                && val.fract() == 0.0
            {
                obj.insert(key.to_string(), Value::Number(serde_json::Number::from(val as i64)));
            }
        }
    }
}

/// Inject default notificationTrigger if absent.
pub fn fixup_default_notification_trigger(value: &mut Value) {
    if value.get("notificationTrigger").is_none()
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("notificationTrigger".to_string(), serde_json::json!(["attributeCreated", "attributeUpdated"]));
    }
}

/// Parse stringified jsonldContext back to structured JSON.
pub fn fixup_jsonld_context(value: &mut Value) {
    let parsed = value.get("jsonldContext").and_then(|v| v.as_str()).and_then(|s| {
        let trimmed = s.trim();
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            serde_json::from_str::<Value>(trimmed).ok()
        } else {
            None
        }
    });
    if let Some(parsed) = parsed
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("jsonldContext".to_string(), parsed);
    }
}

/// Compute expired status at read time based on expiresAt.
pub fn fixup_expired_status(value: &mut Value, expires_at: Option<chrono::DateTime<Utc>>) {
    if let Some(expires_at) = expires_at
        && expires_at <= Utc::now()
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("status".to_string(), Value::String("expired".to_string()));
        obj.insert("isActive".to_string(), Value::Bool(false));
    }
}

/// Compact IRI-bearing fields in a serialized Subscription for HTTP response.
pub async fn compact_subscription_iris(engine: &JsonLdEngine, value: &mut Value, context: &ContextSpec) {
    // Compact entities[].type IRIs
    if let Some(entities) = value.get_mut("entities")
        && let Some(arr) = entities.as_array_mut()
    {
        for entity_sel in arr.iter_mut() {
            if let Some(type_iri) = entity_sel.get("type").and_then(|v| v.as_str()).map(String::from) {
                let compacted = crate::jsonld::term_resolver::compact_type_iri(engine, &type_iri, context).await;
                if let Some(obj) = entity_sel.as_object_mut() {
                    obj.insert("type".to_string(), Value::String(compacted));
                }
            }
        }
    }

    // Compact watchedAttributes IRIs
    if let Some(watched) = value.get_mut("watchedAttributes")
        && let Some(arr) = watched.as_array_mut()
    {
        for attr in arr.iter_mut() {
            if let Some(iri) = attr.as_str().map(String::from) {
                let compacted = crate::jsonld::term_resolver::compact_attr_iri(engine, &iri, context).await;
                *attr = Value::String(compacted);
            }
        }
    }

    // Compact notification.attributes IRIs
    if let Some(notification) = value.get_mut("notification")
        && let Some(attrs) = notification.get_mut("attributes")
        && let Some(arr) = attrs.as_array_mut()
    {
        for attr in arr.iter_mut() {
            if let Some(iri) = attr.as_str().map(String::from) {
                let compacted = crate::jsonld::term_resolver::compact_attr_iri(engine, &iri, context).await;
                *attr = Value::String(compacted);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Field extraction from payload (shared by create)
// ---------------------------------------------------------------------------

/// Helper to extract SubscriptionFields from a payload for create operations.
/// The `dataset_id_override` parameter allows forcing dataset_id to None for CSR subs.
pub fn extract_create_fields<'a>(
    payload: &'a Value,
    watched_attributes_vec: &'a Option<Vec<String>>,
    notification_trigger_vec: &'a Option<Vec<String>>,
    dataset_id_vec: &'a Option<Vec<String>>,
) -> SubscriptionFields<'a> {
    SubscriptionFields {
        subscription_name: payload.get("subscriptionName").and_then(|v| v.as_str()),
        description: payload.get("description").and_then(|v| v.as_str()),
        entities: payload.get("entities"),
        watched_attributes: watched_attributes_vec.as_deref(),
        notification_trigger: notification_trigger_vec.as_deref(),
        time_interval: payload.get("timeInterval").and_then(|v| v.as_f64()),
        q: payload.get("q").and_then(|v| v.as_str()),
        geo_q: payload.get("geoQ"),
        scope_q: payload.get("scopeQ").and_then(|v| v.as_str()),
        csf: payload.get("csf").and_then(|v| v.as_str()),
        dataset_id: dataset_id_vec.as_deref(),
        is_active: payload.get("isActive").and_then(|v| v.as_bool()).unwrap_or(true),
        notification: payload.get("notification"),
        expires_at: payload
            .get("expiresAt")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        throttling: payload.get("throttling").and_then(|v| v.as_f64()),
        temporal_q: payload.get("temporalQ"),
        lang: payload.get("lang").and_then(|v| v.as_str()),
        jsonld_context: payload.get("jsonldContext").and_then(|v| v.as_str()),
        local_only: payload.get("localOnly").and_then(|v| v.as_bool()).unwrap_or(false),
    }
}

/// Owned intermediate values produced by merging an update fragment with an existing subscription.
pub struct MergedUpdateFields {
    /// Optional subscription name.
    pub subscription_name: Option<String>,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Entity selector array.
    pub entities: Option<Value>,
    /// Attributes that trigger notifications on change.
    pub watched_attributes: Option<Vec<String>>,
    /// Events that trigger notifications.
    pub notification_trigger: Option<Vec<String>>,
    /// Periodic notification interval in seconds.
    pub time_interval: Option<f64>,
    /// NGSI-LD query expression.
    pub q: Option<String>,
    /// Geo-spatial query object.
    pub geo_q: Option<Value>,
    /// Scope query expression.
    pub scope_q: Option<String>,
    /// Context source filter expression.
    pub csf: Option<String>,
    /// Dataset IDs to subscribe to.
    pub dataset_id: Option<Vec<String>>,
    /// Whether the subscription is active.
    pub is_active: bool,
    /// Notification configuration object.
    pub notification: Option<Value>,
    /// Expiration timestamp for the subscription.
    pub expires_at: Option<chrono::DateTime<Utc>>,
    /// Minimum interval between notifications in seconds.
    pub throttling: Option<f64>,
    /// Temporal query object.
    pub temporal_q: Option<Value>,
    /// Language filter for notifications.
    pub lang: Option<String>,
    /// JSON-LD context reference for notifications.
    pub jsonld_context: Option<String>,
    /// Whether the subscription applies only to local entities.
    pub local_only: bool,
}

impl MergedUpdateFields {
    /// Convert the merged values into borrowed `SubscriptionFields` for persistence.
    pub fn to_fields(&self) -> SubscriptionFields<'_> {
        SubscriptionFields {
            subscription_name: self.subscription_name.as_deref(),
            description: self.description.as_deref(),
            entities: self.entities.as_ref(),
            watched_attributes: self.watched_attributes.as_deref(),
            notification_trigger: self.notification_trigger.as_deref(),
            time_interval: self.time_interval,
            q: self.q.as_deref(),
            geo_q: self.geo_q.as_ref(),
            scope_q: self.scope_q.as_deref(),
            csf: self.csf.as_deref(),
            dataset_id: self.dataset_id.as_deref(),
            is_active: self.is_active,
            notification: self.notification.as_ref(),
            expires_at: self.expires_at,
            throttling: self.throttling,
            temporal_q: self.temporal_q.as_ref(),
            lang: self.lang.as_deref(),
            jsonld_context: self.jsonld_context.as_deref(),
            local_only: self.local_only,
        }
    }
}

/// Merge payload fragment with existing subscription, returning owned intermediate values.
pub fn merge_update_fields(
    payload: &Value,
    existing: &Subscription,
    existing_value: &Value,
    use_dataset_id: bool,
) -> Result<MergedUpdateFields, AndromedaError> {
    Ok(MergedUpdateFields {
        subscription_name: payload
            .get("subscriptionName")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.subscription_name.clone()), // clone: fallback to existing owned value
        description: payload
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.description.clone()), // clone: fallback to existing owned value
        entities: payload
            .get("entities")
            .cloned() // clone: need owned Value from borrowed payload
            .or_else(|| existing_value.get("entities").cloned()) // clone: fallback to existing owned value
            .filter(|v| !v.is_null()),
        watched_attributes: payload
            .get("watchedAttributes")
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'watchedAttributes': {}", e)))) // clone: serde_json::from_value consumes the Value
            .transpose()?
            .or_else(|| {
                existing
                    .selection
                    .watched_attributes
                    .as_ref()
                    .map(|names| names.iter().map(ToString::to_string).collect())
            }),
        notification_trigger: payload
            .get("notificationTrigger")
            .map(|v| {
                serde_json::from_value::<Vec<String>>(v.clone())
                    .map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'notificationTrigger': {}", e))) // clone: serde_json::from_value consumes the Value
            })
            .transpose()?
            .or_else(|| {
                existing
                    .selection
                    .notification_trigger
                    .as_ref()
                    .map(|t| t.iter().map(|nt| nt.to_string()).collect())
            }),
        time_interval: payload.get("timeInterval").and_then(|v| v.as_f64()).or(existing.delivery.time_interval),
        q: payload
            .get("q")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.filters.q.clone()), // clone: fallback to existing owned value
        geo_q: payload.get("geoQ").cloned().or_else(|| existing.filters.geo_q.clone()).filter(|v| !v.is_null()), // clone: need owned Values from borrowed sources
        scope_q: payload
            .get("scopeQ")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.filters.scope_q.clone()), // clone: fallback to existing owned value
        csf: payload
            .get("csf")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.filters.csf.clone()), // clone: fallback to existing owned value
        dataset_id: if use_dataset_id {
            payload
                .get("datasetId")
                .map(|v| serde_json::from_value(v.clone()).map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'datasetId': {}", e)))) // clone: serde_json::from_value consumes the Value
                .transpose()?
                .or_else(|| existing.filters.dataset_id.as_ref().map(|ids| ids.iter().map(ToString::to_string).collect()))
        } else {
            None
        },
        is_active: payload
            .get("isActive")
            .and_then(|v| v.as_bool())
            .or(existing.lifecycle.is_active)
            .unwrap_or(true),
        notification: payload
            .get("notification")
            .cloned() // clone: need owned Value from borrowed payload
            .or_else(|| {
                serde_json::to_value(&existing.notification)
                    .map_err(|e| tracing::warn!("Failed to serialize existing notification: {}", e))
                    .ok()
            })
            .filter(|v| !v.is_null()),
        expires_at: payload
            .get("expiresAt")
            .and_then(|v| v.as_str())
            .map(|s| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'expiresAt' value '{}': {}", s, e)))
            })
            .transpose()?
            .or(existing.lifecycle.expires_at),
        throttling: payload.get("throttling").and_then(|v| v.as_f64()).or(existing.delivery.throttling),
        temporal_q: payload
            .get("temporalQ")
            .cloned() // clone: need owned Value from borrowed payload
            .or_else(|| existing.filters.temporal_q.clone()) // clone: fallback to existing owned value
            .filter(|v| !v.is_null()),
        lang: payload
            .get("lang")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.delivery.lang.as_ref().map(ToString::to_string)),
        jsonld_context: payload
            .get("jsonldContext")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.delivery.jsonld_context.as_ref().map(ToString::to_string)),
        local_only: payload
            .get("localOnly")
            .and_then(|v| v.as_bool())
            .or(existing.delivery.local_only)
            .unwrap_or(false),
    })
}
