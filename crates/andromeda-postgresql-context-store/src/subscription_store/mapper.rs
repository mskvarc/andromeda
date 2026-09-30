//! Row-to-domain mapping for subscriptions.
//!
//! The PgRow reader lives here because the `subscriptions` table is shared
//! between entity and context-source subscriptions, with a conditionally
//! present `dataset_id` column.

use crate::{
    error::ContextStoreError,
    subscription_store::{
        params::DatasetIdPresence,
        rows::SubscriptionRow,
        status_code::status_of,
        stored_subscription_error::StoredSubscriptionError,
        trigger_code::trigger_of,
    },
};
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReferenceError, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::{
    notification::NotificationTrigger,
    subscription::{
        EntitySelector,
        NotificationParams,
        NotificationStatus,
        Subscription,
        SubscriptionDelivery,
        SubscriptionFilters,
        SubscriptionLifecycle,
        SubscriptionSelection,
        SubscriptionStatus,
    },
};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, scope::query::ScopeQuery};
use andromeda_postgresql_codes::{notification_trigger::NotificationTriggerCode, subscription_status::SubscriptionStatusCode};
use andromeda_postgresql_schema::subscriptions::{SubscriptionDeliveryState, Subscriptions};
use andromeda_subscription::stored_subscription::StoredSubscription;
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use sea_query::Iden;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::{num::NonZeroU32, str::FromStr};
use url::Url;

/// Parse a subscription identifier read from PostgreSQL.
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredSubscription`] when the stored text is not
/// a valid URI, which means the column holds a value the broker never wrote.
pub fn stored_subscription_uri(raw: &str) -> Result<Identifier, ContextStoreError> {
    Ok(raw.parse().map_err(StoredSubscriptionError::Id)?)
}

/// The entity selectors the `entities` column holds.
fn stored_entities(row: &PgRow) -> Result<Option<Vec<EntitySelector>>, ContextStoreError> {
    let entities_json: Option<Value> = row.try_get(Subscriptions::Entities.unquoted())?;
    entities_json
        .map(|value| serde_json::from_value::<Vec<EntitySelector>>(value).map_err(|error| StoredSubscriptionError::Entities(error).into()))
        .transpose()
}

/// The attribute names one `TEXT[]` column of expanded names holds.
///
/// `unreadable` names the column in the failure, so the three columns that hold
/// attribute names are told apart without carrying the column's text.
fn stored_attribute_names(
    row: &PgRow,
    column: Subscriptions,
    unreadable: fn(NgsiLdReferenceError) -> StoredSubscriptionError,
) -> Result<Option<Vec<AttributeName>>, ContextStoreError> {
    let stored: Option<Vec<String>> = row.try_get(column.unquoted())?;
    stored
        .map(|names| names.into_iter().map(|raw| raw.parse().map_err(|error| unreadable(error).into())).collect())
        .transpose()
}

/// The clause 5.3.3 triggers the `notification_trigger` registry codes name.
fn stored_notification_triggers(row: &PgRow) -> Result<Option<Vec<NotificationTrigger>>, ContextStoreError> {
    let trigger_codes: Option<Vec<i16>> = row.try_get(Subscriptions::NotificationTrigger.unquoted())?;
    trigger_codes
        .map(|codes| {
            codes
                .into_iter()
                .map(|code| {
                    NotificationTriggerCode::try_from(code)
                        .map(trigger_of)
                        .map_err(|error| StoredSubscriptionError::NotificationTrigger(error).into())
                })
                .collect()
        })
        .transpose()
}

/// Parse a stored surrogate key into a [`SubscriptionKey`].
fn stored_subscription_key(raw: i64) -> Result<SubscriptionKey, ContextStoreError> {
    Ok(SubscriptionKey::try_from(raw).map_err(StoredSubscriptionError::Key)?)
}

/// Read a `SubscriptionRow` from a `PgRow`, honouring [`DatasetIdPresence`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a projected column is missing or
/// cannot be decoded, and [`ContextStoreError::StoredSubscription`] when a stored value no
/// longer parses into its domain type: the subscription URI, an entity selector,
/// a watched attribute name, a notification trigger code, the `q`, `scopeQ` or
/// `csf` filter, the notification parameters, the language tag, the JSON-LD
/// context URL, or the status code.
pub fn row_from_pg(row: &PgRow, dataset_id_presence: DatasetIdPresence) -> Result<SubscriptionRow, ContextStoreError> {
    let dataset_id_raw: Option<Vec<String>> = match dataset_id_presence {
        DatasetIdPresence::Present => row.try_get(Subscriptions::DatasetId.unquoted())?,
        DatasetIdPresence::Absent => None,
    };
    let dataset_id = dataset_id_raw
        .map(|ids| {
            ids.iter()
                .map(|raw| DatasetSelector::parse(raw).map_err(|error| StoredSubscriptionError::DatasetId(Box::new(error))))
                .collect::<Result<Vec<DatasetSelector>, StoredSubscriptionError>>()
        })
        .transpose()?;
    let id = stored_subscription_uri(row.try_get(Subscriptions::Id.unquoted())?)?;
    let entities = stored_entities(row)?;
    let watched_attributes = stored_attribute_names(row, Subscriptions::WatchedAttributes, StoredSubscriptionError::WatchedAttribute)?;
    let notification_trigger = stored_notification_triggers(row)?;
    let q_raw: Option<String> = row.try_get(Subscriptions::Q.unquoted())?;
    let q = q_raw
        .map(|raw| raw.parse().map_err(|error| StoredSubscriptionError::Q(Box::new(error))))
        .transpose()?;
    let scope_q_raw: Option<String> = row.try_get(Subscriptions::ScopeQ.unquoted())?;
    let scope_q = scope_q_raw
        .map(|raw| ScopeQuery::parse(&raw).map_err(|error| StoredSubscriptionError::ScopeQ(Box::new(error))))
        .transpose()?;
    let csf_raw: Option<String> = row.try_get(Subscriptions::Csf.unquoted())?;
    let csf = csf_raw
        .map(|raw| raw.parse().map_err(|error| StoredSubscriptionError::Csf(Box::new(error))))
        .transpose()?;
    let expand_values = stored_attribute_names(row, Subscriptions::ExpandValues, StoredSubscriptionError::ExpandValue)?;
    let json_keys = stored_attribute_names(row, Subscriptions::JsonKeys, StoredSubscriptionError::JsonKey)?;
    let notification_json: Value = row.try_get(Subscriptions::Notification.unquoted())?;
    let notification = serde_json::from_value(notification_json).map_err(StoredSubscriptionError::Notification)?;
    let lang_raw: Option<String> = row.try_get(Subscriptions::Lang.unquoted())?;
    let lang = lang_raw.map(LangTagBuf::new).transpose().map_err(StoredSubscriptionError::Lang)?;
    let jsonld_context_raw: Option<String> = row.try_get(Subscriptions::JsonldContext.unquoted())?;
    let jsonld_context = jsonld_context_raw
        .map(|raw| Url::parse(&raw).map_err(StoredSubscriptionError::JsonldContext))
        .transpose()?;
    let status = SubscriptionStatusCode::try_from(row.try_get::<i16, _>(Subscriptions::Status.unquoted())?)
        .map(status_of)
        .map_err(StoredSubscriptionError::Status)?;
    let ngsild_conformance_raw: Option<String> = row.try_get(Subscriptions::NgsildConformance.unquoted())?;
    let ngsild_conformance = ngsild_conformance_raw
        .map(|raw| SpecVersion::from_str(&raw).map_err(StoredSubscriptionError::NgsildConformance))
        .transpose()?;
    Ok(SubscriptionRow {
        id,
        key: stored_subscription_key(row.try_get(Subscriptions::SubscriptionKey.unquoted())?)?,
        subscription_name: row.try_get(Subscriptions::SubscriptionName.unquoted())?,
        description: row.try_get(Subscriptions::Description.unquoted())?,
        entities,
        watched_attributes,
        notification_trigger,
        time_interval: row.try_get(Subscriptions::TimeInterval.unquoted())?,
        q,
        geo_q: row.try_get(Subscriptions::GeoQ.unquoted())?,
        scope_q,
        csf,
        expand_values,
        json_keys,
        dataset_id,
        notification,
        expires_at: row.try_get(Subscriptions::ExpiresAt.unquoted())?,
        throttling: row.try_get(Subscriptions::Throttling.unquoted())?,
        status,
        temporal_q: row.try_get(Subscriptions::TemporalQ.unquoted())?,
        lang,
        jsonld_context,
        local_only: row.try_get(Subscriptions::LocalOnly.unquoted())?,
        split_entities: row.try_get(Subscriptions::SplitEntities.unquoted())?,
        ngsild_conformance,
        times_sent: row.try_get(SubscriptionDeliveryState::TimesSent.unquoted())?,
        times_failed: row.try_get(SubscriptionDeliveryState::TimesFailed.unquoted())?,
        last_notification: row.try_get(SubscriptionDeliveryState::LastNotification.unquoted())?,
        last_success: row.try_get(SubscriptionDeliveryState::LastSuccess.unquoted())?,
        last_failure: row.try_get(SubscriptionDeliveryState::LastFailure.unquoted())?,
        created_at: row.try_get(Subscriptions::CreatedAt.unquoted())?,
        modified_at: row.try_get(Subscriptions::ModifiedAt.unquoted())?,
    })
}

/// Map a whole result set, failing the read on the first row that does not map.
///
/// A row this build cannot decode is a hard error rather than an omission: the
/// listing's total is counted by a separate query, so dropping a row would make
/// the page disagree with its own count, and `list_active` feeds the matching
/// cache, where an omitted subscription silently stops firing.
///
/// # Errors
///
/// Returns the [`ContextStoreError`] of the first row whose stored columns do
/// not decode back into a [`SubscriptionRow`].
pub fn rows_to_stored_subscriptions(rows: &[PgRow], dataset_id_presence: DatasetIdPresence) -> Result<Vec<StoredSubscription>, ContextStoreError> {
    let mut subscriptions = Vec::with_capacity(rows.len());
    for pg_row in rows {
        subscriptions.push(row_to_stored_subscription(row_from_pg(pg_row, dataset_id_presence)?));
    }
    Ok(subscriptions)
}

/// Convert a persistence-shaped `SubscriptionRow` into the resource paired with
/// the key it is stored under.
#[must_use]
pub fn row_to_stored_subscription(row: SubscriptionRow) -> StoredSubscription {
    let key = row.key;
    StoredSubscription {
        key,
        subscription: row_to_subscription(row),
    }
}

/// The member a delivery counter renders as (Table 5.2.14.2-1).
///
/// The column floors at zero because a counter has to be somewhere before it is
/// first advanced; the member is restricted to "Greater than 0", so a counter
/// still at rest is absent. A negative count is not reachable through the
/// column's own `CHECK`, and is read the same way as a counter at rest.
fn positive_counter(counted: i32) -> Option<NonZeroU32> {
    u32::try_from(counted).ok().and_then(NonZeroU32::new)
}

/// Convert a persistence-shaped `SubscriptionRow` into the domain [`Subscription`].
///
/// Parses JSON columns into typed domain values and derives the notification
/// delivery status from the `last_success`/`last_failure` timestamps.
fn row_to_subscription(row: SubscriptionRow) -> Subscription {
    let mut notification: NotificationParams = row.notification;

    // Clause 5.2.14.2 makes these output only: whatever a client stored in the
    // notification JSONB is replaced by the counters the delivery state holds.
    // Table 5.2.14.2-1 restricts both counters to "Greater than 0" at
    // cardinality 0..1, so one that has not moved is an absent member rather
    // than a zero the client has to interpret.
    notification.times_sent = positive_counter(row.times_sent);
    notification.times_failed = positive_counter(row.times_failed);
    notification.last_notification = row.last_notification;
    notification.last_success = row.last_success;
    notification.last_failure = row.last_failure;
    notification.status = notification_status_from_timestamps(row.last_success, row.last_failure);

    Subscription::builder()
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
                .expand_values(row.expand_values)
                .json_keys(row.json_keys)
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
                .ngsild_conformance(row.ngsild_conformance)
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
        .build()
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
