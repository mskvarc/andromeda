//! Row-to-domain mapping for subscriptions.
//!
//! The PgRow reader lives here because the `subscriptions` table is shared
//! between entity and context-source subscriptions, with a conditionally
//! present `dataset_id` column.

use crate::{
    error::ContextStoreError,
    notification_delivery::trigger_code::trigger_of,
    subscription_store::{params::DatasetIdPresence, rows::SubscriptionRow},
};
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::{
    EntitySelector,
    NotificationParams,
    NotificationStatus,
    Subscription,
    SubscriptionDelivery,
    SubscriptionFilters,
    SubscriptionLifecycle,
    SubscriptionSelection,
    SubscriptionStatus,
};
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
use andromeda_postgresql_codes::notification_trigger::NotificationTriggerCode;
use andromeda_subscription::stored_subscription::StoredSubscription;
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use tracing::warn;
use url::Url;

/// Parse a subscription identifier read from PostgreSQL.
pub fn stored_subscription_uri(raw: String) -> Result<Uri, ContextStoreError> {
    raw.parse()
        .map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription id '{raw}': {error}")))
}

/// Parse a stored surrogate key into a [`SubscriptionKey`].
fn stored_subscription_key(raw: i64) -> Result<SubscriptionKey, ContextStoreError> {
    SubscriptionKey::try_from(raw).map_err(|error| ContextStoreError::Internal(format!("stored subscription key is not a valid surrogate key: {error}")))
}

/// Read a `SubscriptionRow` from a `PgRow`, honouring [`DatasetIdPresence`].
pub fn row_from_pg(row: &PgRow, dataset_id_presence: DatasetIdPresence) -> Result<SubscriptionRow, ContextStoreError> {
    let dataset_id_raw: Option<Vec<String>> = match dataset_id_presence {
        DatasetIdPresence::Present => row.try_get("dataset_id")?,
        DatasetIdPresence::Absent => None,
    };
    let dataset_id = dataset_id_raw.map(|ids| ids.into_iter().map(stored_subscription_uri).collect()).transpose()?;
    let id = stored_subscription_uri(row.try_get("id")?)?;
    let entities_json: Option<Value> = row.try_get("entities")?;
    let entities = entities_json
        .map(|value| {
            serde_json::from_value::<Vec<EntitySelector>>(value)
                .map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription entities: {error}")))
        })
        .transpose()?;
    let watched_attribute_names: Option<Vec<String>> = row.try_get("watched_attributes")?;
    let watched_attributes = watched_attribute_names
        .map(|names| {
            names
                .into_iter()
                .map(|raw| {
                    raw.parse()
                        .map_err(|error| ContextStoreError::Internal(format!("Invalid stored watched attribute '{raw}': {error}")))
                })
                .collect()
        })
        .transpose()?;
    let trigger_codes: Option<Vec<i16>> = row.try_get("notification_trigger")?;
    let notification_trigger = trigger_codes
        .map(|codes| {
            codes
                .into_iter()
                .map(|code| {
                    NotificationTriggerCode::try_from(code)
                        .map(trigger_of)
                        .map_err(|error| ContextStoreError::Internal(format!("Invalid stored notification trigger code {code}: {error}")))
                })
                .collect()
        })
        .transpose()?;
    let q_raw: Option<String> = row.try_get("q")?;
    let q = q_raw
        .map(|raw| {
            raw.parse()
                .map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription q filter '{raw}': {error}")))
        })
        .transpose()?;
    let scope_q_raw: Option<String> = row.try_get("scope_q")?;
    let scope_q = scope_q_raw
        .map(|raw| ScopeQuery::parse(&raw).map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription scopeQ '{raw}': {error}"))))
        .transpose()?;
    let csf_raw: Option<String> = row.try_get("csf")?;
    let csf = csf_raw
        .map(|raw| {
            raw.parse()
                .map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription csf filter '{raw}': {error}")))
        })
        .transpose()?;
    let notification_json: Value = row.try_get("notification")?;
    let notification =
        serde_json::from_value(notification_json).map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription notification: {error}")))?;
    let lang_raw: Option<String> = row.try_get("lang")?;
    let lang = lang_raw
        .map(LangTagBuf::new)
        .transpose()
        .map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription lang: {error}")))?;
    let jsonld_context_raw: Option<String> = row.try_get("jsonld_context")?;
    let jsonld_context = jsonld_context_raw
        .map(|raw| Url::parse(&raw).map_err(|error| ContextStoreError::Internal(format!("Invalid stored subscription jsonldContext '{raw}': {error}"))))
        .transpose()?;
    let status_label: String = row.try_get("status")?;
    Ok(SubscriptionRow {
        id,
        key: stored_subscription_key(row.try_get("subscription_key")?)?,
        subscription_name: row.try_get("subscription_name")?,
        description: row.try_get("description")?,
        entities,
        watched_attributes,
        notification_trigger,
        time_interval: row.try_get("time_interval")?,
        q,
        geo_q: row.try_get("geo_q")?,
        scope_q,
        csf,
        dataset_id,
        notification,
        expires_at: row.try_get("expires_at")?,
        throttling: row.try_get("throttling")?,
        status: status_label
            .parse()
            .map_err(|e| ContextStoreError::Internal(format!("Invalid subscription status '{status_label}': {e}")))?,
        temporal_q: row.try_get("temporal_q")?,
        lang,
        jsonld_context,
        local_only: row.try_get("local_only")?,
        split_entities: row.try_get("split_entities")?,
        times_sent: row.try_get("times_sent")?,
        last_notification: row.try_get("last_notification")?,
        last_success: row.try_get("last_success")?,
        last_failure: row.try_get("last_failure")?,
        created_at: row.try_get("created_at")?,
        modified_at: row.try_get("modified_at")?,
    })
}

/// Map every row that maps, and omit the ones that do not.
///
/// The resource holds its filters and selectors parsed, so a row written by an
/// older build -- or edited by hand -- can carry a value this build's grammars
/// reject. Failing the whole read for one of them takes every other
/// subscription with it: a listing becomes a 500, and `list_active` empties the
/// matching cache so no subscription fires at all. A multi-row read therefore
/// reports what it can read and logs what it cannot, while
/// [`row_to_stored_subscription`] keeps its hard error for `get`, where the
/// client named that one subscription and has to be told it is unreadable.
///
/// A listing's total still counts the stored rows, so it can exceed the number
/// returned. That is the honest figure: the rows are there, this build cannot
/// read them.
pub fn rows_to_stored_subscriptions(rows: &[PgRow], dataset_id_presence: DatasetIdPresence) -> Result<Vec<StoredSubscription>, ContextStoreError> {
    let mut subscriptions = Vec::with_capacity(rows.len());
    for pg_row in rows {
        let row = row_from_pg(pg_row, dataset_id_presence)?;
        // clone: the identifier is needed for the log after `row` is consumed,
        // and it is the only thing that names the row that failed
        let id = row.id.clone();
        match row_to_stored_subscription(row) {
            Ok(stored) => subscriptions.push(stored),
            Err(error) => warn!(subscription_id = %id, %error, "subscription row could not be read and was omitted"),
        }
    }
    Ok(subscriptions)
}

/// Convert a persistence-shaped `SubscriptionRow` into the resource paired with
/// the key it is stored under.
pub fn row_to_stored_subscription(row: SubscriptionRow) -> Result<StoredSubscription, ContextStoreError> {
    let key = row.key;
    Ok(StoredSubscription {
        key,
        subscription: row_to_subscription(row)?,
    })
}

/// Convert a persistence-shaped `SubscriptionRow` into the domain [`Subscription`].
///
/// Parses JSON columns into typed domain values and derives the notification
/// delivery status from the `last_success`/`last_failure` timestamps.
fn row_to_subscription(row: SubscriptionRow) -> Result<Subscription, ContextStoreError> {
    let mut notification: NotificationParams = row.notification;

    notification.times_sent = Some(row.times_sent);
    notification.last_notification = row.last_notification;
    notification.last_success = row.last_success;
    notification.last_failure = row.last_failure;
    notification.status = notification_status_from_timestamps(row.last_success, row.last_failure);

    Ok(Subscription::builder()
        .id(row.id)
        .subscription_name(row.subscription_name)
        .description(row.description)
        .selection(
            SubscriptionSelection::builder()
                .entities(row.entities)
                .watched_attributes(row.watched_attributes)
                .notification_trigger(row.notification_trigger)
                .build(),
        )
        .filters(
            SubscriptionFilters::builder()
                .q(row.q)
                .geo_q(row.geo_q)
                .scope_q(row.scope_q)
                .csf(row.csf)
                .dataset_id(row.dataset_id)
                .temporal_q(row.temporal_q)
                .build(),
        )
        .notification(notification)
        .delivery(
            SubscriptionDelivery::builder()
                .time_interval(row.time_interval)
                .throttling(row.throttling)
                .lang(row.lang)
                .jsonld_context(row.jsonld_context)
                .local_only(if row.local_only { Some(true) } else { None })
                .split_entities(row.split_entities)
                .build(),
        )
        .lifecycle(
            SubscriptionLifecycle::builder()
                // `isActive` is no longer stored: on read it is always
                // `status == active`, so it is derived rather than kept.
                .is_active(Some(row.status == SubscriptionStatus::Active))
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
