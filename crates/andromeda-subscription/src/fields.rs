//! Typed subscription persistence values and update merging.

use crate::{error::SubscriptionError, lifecycle::DatasetIdPolicy, store::SubscriptionFields};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    notification::{DEFAULT_NOTIFICATION_TRIGGERS, NotificationTrigger},
    subscription::{EntitySelector, NotificationParams, Subscription},
};
use andromeda_ngsi_ld_query::{filter::expression::Expr, scope::query::ScopeQuery};
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

/// Owned, validated values for one subscription persistence write.
///
/// This shape mirrors the subscription table boundary. Domain-valued members
/// have already been parsed; only human-readable text and opaque JSON members
/// remain textual.
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
    /// Geo-spatial query object.
    pub geo_q: Option<Value>,
    /// Parsed scope query expression.
    pub scope_q: Option<ScopeQuery>,
    /// Parsed context-source filter expression.
    pub csf: Option<Expr>,
    /// Validated dataset identifiers.
    pub dataset_id: Option<Vec<Uri>>,
    /// Whether the subscription is currently active.
    pub is_active: bool,
    /// Validated notification delivery parameters.
    pub notification: Option<NotificationParams>,
    /// Expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Minimum interval between notifications in seconds.
    pub throttling: Option<f64>,
    /// Temporal query object.
    pub temporal_q: Option<Value>,
    /// Validated preferred language.
    pub lang: Option<LangTagBuf>,
    /// Validated JSON-LD context URL.
    pub jsonld_context: Option<Url>,
    /// Whether the subscription applies only to local entities.
    pub local_only: bool,
}

impl SubscriptionWriteValues {
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
            lang: self.lang.as_ref(),
            jsonld_context: self.jsonld_context.as_ref(),
            local_only: self.local_only,
        }
    }
}

fn deserialize_member<T>(payload: &Value, member: &str) -> Result<Option<T>, SubscriptionError>
where
    T: DeserializeOwned,
{
    payload
        .get(member)
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|error| {
                // clone: serde owns the selected JSON member while the request
                // payload remains available to the other field parsers.
                SubscriptionError::validation(format!("Invalid '{member}' value: {error}"))
            })
        })
        .transpose()
}

fn expiration(payload: &Value) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
    payload
        .get("expiresAt")
        .and_then(Value::as_str)
        .map(|raw| {
            DateTime::parse_from_rfc3339(raw)
                .map(|date_time| date_time.with_timezone(&Utc))
                .map_err(|error| SubscriptionError::validation(format!("Invalid 'expiresAt' value '{raw}': {error}")))
        })
        .transpose()
}

/// Parse a create payload into validated persistence values.
pub fn extract_create_fields(payload: &Value, dataset_id_policy: DatasetIdPolicy) -> Result<SubscriptionWriteValues, SubscriptionError> {
    Ok(SubscriptionWriteValues {
        subscription_name: payload.get("subscriptionName").and_then(Value::as_str).map(String::from),
        description: payload.get("description").and_then(Value::as_str).map(String::from),
        entities: deserialize_member(payload, "entities")?,
        watched_attributes: deserialize_member(payload, "watchedAttributes")?,
        notification_trigger: Some(deserialize_member(payload, "notificationTrigger")?.unwrap_or_else(|| DEFAULT_NOTIFICATION_TRIGGERS.to_vec())),
        time_interval: payload.get("timeInterval").and_then(Value::as_f64),
        q: deserialize_member(payload, "q")?,
        geo_q: payload.get("geoQ").cloned(), // clone: the write values own request JSON across the database await.
        scope_q: deserialize_member(payload, "scopeQ")?,
        csf: deserialize_member(payload, "csf")?,
        dataset_id: match dataset_id_policy {
            DatasetIdPolicy::Retain => deserialize_member(payload, "datasetId")?,
            DatasetIdPolicy::Discard => None,
        },
        is_active: payload.get("isActive").and_then(Value::as_bool).unwrap_or(true),
        notification: deserialize_member(payload, "notification")?,
        expires_at: expiration(payload)?,
        throttling: payload.get("throttling").and_then(Value::as_f64),
        temporal_q: payload.get("temporalQ").cloned(), // clone: the write values own request JSON across the database await.
        lang: deserialize_member(payload, "lang")?,
        jsonld_context: deserialize_member(payload, "jsonldContext")?,
        local_only: payload.get("localOnly").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Merge an update fragment with an existing subscription into validated
/// persistence values.
pub fn merge_update_fields(payload: &Value, existing: Subscription, dataset_id_policy: DatasetIdPolicy) -> Result<SubscriptionWriteValues, SubscriptionError> {
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

    Ok(SubscriptionWriteValues {
        subscription_name: payload.get("subscriptionName").and_then(Value::as_str).map(String::from).or(subscription_name),
        description: payload.get("description").and_then(Value::as_str).map(String::from).or(description),
        entities: deserialize_member(payload, "entities")?.or(selection.entities),
        watched_attributes: deserialize_member(payload, "watchedAttributes")?.or(selection.watched_attributes),
        notification_trigger: Some(
            deserialize_member(payload, "notificationTrigger")?
                .or(selection.notification_trigger)
                .unwrap_or_else(|| DEFAULT_NOTIFICATION_TRIGGERS.to_vec()),
        ),
        time_interval: payload.get("timeInterval").and_then(Value::as_f64).or(delivery.time_interval),
        q: deserialize_member(payload, "q")?.or(filters.q),
        geo_q: payload
            .get("geoQ")
            .cloned() // clone: the merged values own a member borrowed from the update payload.
            .or(filters.geo_q)
            .filter(|value| !value.is_null()),
        scope_q: deserialize_member(payload, "scopeQ")?.or(filters.scope_q),
        csf: deserialize_member(payload, "csf")?.or(filters.csf),
        dataset_id: match dataset_id_policy {
            DatasetIdPolicy::Retain => deserialize_member(payload, "datasetId")?.or(filters.dataset_id),
            DatasetIdPolicy::Discard => None,
        },
        is_active: payload.get("isActive").and_then(Value::as_bool).or(lifecycle.is_active).unwrap_or(true),
        notification: deserialize_member(payload, "notification")?.or(Some(notification)),
        expires_at: expiration(payload)?.or(lifecycle.expires_at),
        throttling: payload.get("throttling").and_then(Value::as_f64).or(delivery.throttling),
        temporal_q: payload
            .get("temporalQ")
            .cloned() // clone: the merged values own a member borrowed from the update payload.
            .or(filters.temporal_q)
            .filter(|value| !value.is_null()),
        lang: deserialize_member(payload, "lang")?.or(delivery.lang),
        jsonld_context: deserialize_member(payload, "jsonldContext")?.or(delivery.jsonld_context),
        local_only: payload.get("localOnly").and_then(Value::as_bool).or(delivery.local_only).unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use crate::{error::SubscriptionError, fields::extract_create_fields, lifecycle::DatasetIdPolicy};

    #[test]
    fn entity_subscription_creation_retains_dataset_identifiers() -> Result<(), SubscriptionError> {
        let values = extract_create_fields(&serde_json::json!({"datasetId": ["urn:ngsi-ld:Dataset:one"]}), DatasetIdPolicy::Retain)?;

        assert_eq!(
            values
                .dataset_id
                .as_deref()
                .and_then(|identifiers| identifiers.first())
                .map(AsRef::<str>::as_ref),
            Some("urn:ngsi-ld:Dataset:one")
        );
        Ok(())
    }

    #[test]
    fn context_source_subscription_creation_discards_dataset_identifiers() -> Result<(), SubscriptionError> {
        let values = extract_create_fields(&serde_json::json!({"datasetId": ["urn:ngsi-ld:Dataset:one"]}), DatasetIdPolicy::Discard)?;

        assert!(values.dataset_id.is_none());
        Ok(())
    }

    #[test]
    fn malformed_expiration_is_rejected() {
        let result = extract_create_fields(&serde_json::json!({"expiresAt": "tomorrow"}), DatasetIdPolicy::Retain);
        assert!(matches!(result, Err(SubscriptionError::Validation { .. })));
    }
}
