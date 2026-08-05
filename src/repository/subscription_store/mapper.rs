//! Row-to-domain mapping for subscriptions.
//!
//! The PgRow reader lives here because the `subscriptions` table is shared
//! between entity and context-source subscriptions, with a conditionally
//! present `dataset_id` column.

use crate::{
    core::error::AndromedaError,
    repository::subscription_store::{params::DatasetIdPresence, rows::SubscriptionRow},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::{
    NotificationParams,
    NotificationStatus,
    NotificationTrigger,
    Subscription,
    SubscriptionDelivery,
    SubscriptionFilters,
    SubscriptionLifecycle,
    SubscriptionSelection,
};
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use sqlx::Row;
use tracing::warn;
use url::Url;

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

/// Map every row that maps, and omit the ones that do not.
///
/// The resource holds its filters and selectors parsed, so a row written by an
/// older build — or edited by hand — can carry a value this build's grammars
/// reject. Failing the whole read for one of them takes every other
/// subscription with it: a listing becomes a 500, and `list_active` empties the
/// matching cache so no subscription fires at all. A multi-row read therefore
/// reports what it can read and logs what it cannot, while
/// [`row_to_subscription`] keeps its hard error for `get`, where the client
/// named that one subscription and has to be told it is unreadable.
///
/// A listing's total still counts the stored rows, so it can exceed the number
/// returned. That is the honest figure: the rows are there, this build cannot
/// read them.
pub fn rows_to_subscriptions(rows: &[sqlx::postgres::PgRow], dataset_id_presence: DatasetIdPresence) -> Result<Vec<Subscription>, AndromedaError> {
    let mut subscriptions = Vec::with_capacity(rows.len());
    for pg_row in rows {
        let row = row_from_pg(pg_row, dataset_id_presence)?;
        // clone: the identifier is needed for the log after `row` is consumed,
        // and it is the only thing that names the row that failed
        let id = row.id.clone();
        match row_to_subscription(row) {
            Ok(subscription) => subscriptions.push(subscription),
            Err(error) => warn!(subscription_id = %id, %error, "subscription row could not be read and was omitted"),
        }
    }
    Ok(subscriptions)
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

    let id: Uri = row
        .id
        .parse()
        .map_err(|e| AndromedaError::internal(format!("Invalid subscription id '{}': {e}", row.id)))?;
    let watched_attributes = row
        .watched_attributes
        .map(|names| names.iter().map(|raw| raw.parse()).collect::<Result<Vec<_>, _>>())
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid watched attribute name: {e}")))?;
    let dataset_id = row
        .dataset_id
        .map(|ids| ids.iter().map(|raw| raw.parse()).collect::<Result<Vec<_>, _>>())
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid subscription datasetId: {e}")))?;
    let lang = row
        .lang
        .map(LangTagBuf::new)
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid subscription lang: {e}")))?;
    let jsonld_context = row
        .jsonld_context
        .as_deref()
        .map(Url::parse)
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid subscription jsonldContext: {e}")))?;

    // The three filter columns are text, and the resource holds them parsed. A
    // row that will not parse was written by an older build or edited by hand;
    // either way it cannot be matched against, so it is an error here rather
    // than a subscription that silently never fires.
    let q = row
        .q
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid stored subscription q filter: {e}")))?;
    let scope_q = row
        .scope_q
        .as_deref()
        .map(ScopeQuery::parse)
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid stored subscription scopeQ: {e}")))?;
    let csf = row
        .csf
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid stored subscription csf filter: {e}")))?;

    Ok(Subscription::builder()
        .id(id)
        .subscription_name(row.subscription_name)
        .description(row.description)
        .selection(
            SubscriptionSelection::builder()
                .entities(entities)
                .watched_attributes(watched_attributes)
                .notification_trigger(notification_trigger)
                .build(),
        )
        .filters(
            SubscriptionFilters::builder()
                .q(q)
                .geo_q(row.geo_q)
                .scope_q(scope_q)
                .csf(csf)
                .dataset_id(dataset_id)
                .temporal_q(row.temporal_q)
                .build(),
        )
        .notification(notification)
        .delivery(
            SubscriptionDelivery::builder()
                .time_interval(row.time_interval)
                .throttling(row.throttling)
                .lang(lang)
                .jsonld_context(jsonld_context)
                .local_only(if row.local_only { Some(true) } else { None })
                .build(),
        )
        .lifecycle(
            SubscriptionLifecycle::builder()
                .is_active(Some(row.is_active))
                .status(row.status)
                .expires_at(row.expires_at)
                .created_at(row.created_at)
                .modified_at(row.modified_at)
                .build(),
        )
        .build())
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
