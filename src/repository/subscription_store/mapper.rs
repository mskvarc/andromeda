//! Row-to-domain mapping for subscriptions.
//!
//! The PgRow reader lives here because the `subscriptions` table is shared
//! between entity and context-source subscriptions, with a conditionally
//! present `dataset_id` column.

use crate::{
    core::error::AndromedaError,
    domain::subscription::{NotificationParams, NotificationStatus, NotificationTrigger, Subscription},
    repository::subscription_store::{params::DatasetIdPresence, rows::SubscriptionRow},
};
use chrono::{DateTime, Utc};
use sqlx::Row;

/// Read a `SubscriptionRow` from a `PgRow`, honouring [`DatasetIdPresence`].
pub fn row_from_pg(row: &sqlx::postgres::PgRow, dataset_id_presence: DatasetIdPresence) -> Result<SubscriptionRow, AndromedaError> {
    let dataset_id: Option<Vec<String>> = match dataset_id_presence {
        DatasetIdPresence::Present => row.get("dataset_id"),
        DatasetIdPresence::Absent => None,
    };
    let local_only: bool = match row.try_get("local_only") {
        Ok(v) => v,
        Err(sqlx::Error::ColumnNotFound(_)) | Err(sqlx::Error::ColumnDecode { .. }) => false,
        Err(e) => return Err(AndromedaError::internal(format!("Failed to read local_only column: {e}"))),
    };
    // `status` is projected as `status::text`; the label mapping lives in the
    // domain's `EnumString`, so a row carrying a label this build does not know
    // becomes a typed parse error instead of an opaque sqlx decode failure.
    let status_label: String = row.get("status");
    Ok(SubscriptionRow {
        id: row.get("id"),
        type_: row.get("type"),
        subscription_name: row.get("subscription_name"),
        description: row.get("description"),
        entities_json: row.get("entities"),
        watched_attributes: row.get("watched_attributes"),
        notification_trigger_strs: row.get("notification_trigger"),
        time_interval: row.get("time_interval"),
        q: row.get("q"),
        geo_q: row.get("geo_q"),
        scope_q: row.get("scope_q"),
        csf: row.get("csf"),
        dataset_id,
        is_active: row.get("is_active"),
        notification_json: row.get("notification"),
        expires_at: row.get("expires_at"),
        throttling: row.get("throttling"),
        status: status_label
            .parse()
            .map_err(|e| AndromedaError::internal(format!("Invalid subscription status '{status_label}': {e}")))?,
        temporal_q: row.get("temporal_q"),
        lang: row.get("lang"),
        jsonld_context: row.get("jsonld_context"),
        local_only,
        times_sent: row.get("times_sent"),
        last_notification: row.get("last_notification"),
        last_success: row.get("last_success"),
        last_failure: row.get("last_failure"),
        created_at: row.get("created_at"),
        modified_at: row.get("modified_at"),
    })
}

/// Convert a persistence-shaped `SubscriptionRow` into the domain [`Subscription`].
///
/// Parses JSON columns into typed domain values and derives the notification
/// delivery status from the `last_success`/`last_failure` timestamps.
pub fn row_to_subscription(row: SubscriptionRow) -> Result<Subscription, AndromedaError> {
    let notification_trigger: Option<Vec<NotificationTrigger>> = row.notification_trigger_strs.map(|v| v.into_iter().filter_map(|s| s.parse().ok()).collect());

    let entities = row
        .entities_json
        .map(|v| serde_json::from_value(v).map_err(|e| AndromedaError::internal(format!("Failed to parse entities: {e}"))))
        .transpose()?;

    let mut notification: NotificationParams =
        serde_json::from_value(row.notification_json).map_err(|e| AndromedaError::internal(format!("Failed to parse notification: {e}")))?;

    notification.times_sent = Some(row.times_sent);
    notification.last_notification = row.last_notification;
    notification.last_success = row.last_success;
    notification.last_failure = row.last_failure;
    notification.status = notification_status_from_timestamps(row.last_success, row.last_failure);

    Ok(Subscription {
        id: row.id,
        type_: row.type_,
        subscription_name: row.subscription_name,
        description: row.description,
        entities,
        watched_attributes: row.watched_attributes,
        notification_trigger,
        time_interval: row.time_interval,
        q: row.q,
        geo_q: row.geo_q,
        scope_q: row.scope_q,
        csf: row.csf,
        dataset_id: row.dataset_id,
        is_active: Some(row.is_active),
        notification,
        expires_at: row.expires_at,
        throttling: row.throttling,
        status: row.status,
        temporal_q: row.temporal_q,
        lang: row.lang,
        jsonld_context: row.jsonld_context,
        local_only: if row.local_only { Some(true) } else { None },
        created_at: row.created_at,
        modified_at: row.modified_at,
    })
}

/// Derive the notification delivery status from `last_success`/`last_failure`.
fn notification_status_from_timestamps(last_success: Option<DateTime<Utc>>, last_failure: Option<DateTime<Utc>>) -> Option<NotificationStatus> {
    match (last_success, last_failure) {
        (Some(s), Some(f)) => Some(if s >= f { NotificationStatus::Ok } else { NotificationStatus::Failed }),
        (Some(_), None) => Some(NotificationStatus::Ok),
        (None, Some(_)) => Some(NotificationStatus::Failed),
        (None, None) => None,
    }
}
