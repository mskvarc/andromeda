//! Subscription persistence field extraction and update merging.

use crate::service::subscription_core::{AndromedaError, Deserialize, EntitySelector, Expr, ScopeQuery, Subscription, SubscriptionFields, Utc, Value};

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

/// Reject a subscription whose stored form this build could not read back.
///
/// Every filter and selector member of the resource is a parsed type, so a row
/// a write produces has to satisfy the same grammars the read applies. Checking
/// it here makes a bad value a 400 on the request that introduced it; without
/// it the write succeeds against columns that are plain text and JSON, and the
/// failure surfaces later as a 500 on a read of a row nobody can now attribute
/// to a request.
///
/// This is the boundary rejection clause 5.5.2 requires for `q` (4.9),
/// `scopeQ` (4.18), `csf` (4.13) and an `EntitySelector`'s `id`, `idPattern`
/// and `type` (5.2.11).
pub fn validate_persisted_shape(fields: &SubscriptionFields<'_>) -> Result<(), AndromedaError> {
    if let Some(entities) = fields.entities {
        Vec::<EntitySelector>::deserialize(entities).map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'entities': {e}")))?;
    }
    if let Some(q) = fields.q {
        q.parse::<Expr>()
            .map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'q': {e}")))?;
    }
    if let Some(scope_q) = fields.scope_q {
        ScopeQuery::parse(scope_q).map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'scopeQ': {e}")))?;
    }
    if let Some(csf) = fields.csf {
        csf.parse::<Expr>()
            .map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'csf': {e}")))?;
    }
    Ok(())
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
            .or_else(|| existing.filters.q.as_ref().map(ToString::to_string)),
        geo_q: payload.get("geoQ").cloned().or_else(|| existing.filters.geo_q.clone()).filter(|v| !v.is_null()), // clone: need owned Values from borrowed sources
        scope_q: payload
            .get("scopeQ")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.filters.scope_q.as_ref().map(ToString::to_string)),
        csf: payload
            .get("csf")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.filters.csf.as_ref().map(ToString::to_string)),
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
