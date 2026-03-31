use crate::{
    domain::{
        subscription::{NotificationParams, NotificationStatus, Subscription, SubscriptionStatus},
        uri::NGSI_LD_URN_NID,
    },
    error::AndromedaError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;
use urn::UrnBuilder;
use uuid::Uuid;

pub struct SubscriptionFields<'a> {
    pub subscription_name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub entities: Option<&'a Value>,
    pub watched_attributes: Option<&'a [String]>,
    pub notification_trigger: Option<&'a [String]>,
    pub time_interval: Option<f64>,
    pub q: Option<&'a str>,
    pub geo_q: Option<&'a Value>,
    pub scope_q: Option<&'a str>,
    pub csf: Option<&'a str>,
    pub dataset_id: Option<&'a [String]>,
    pub is_active: bool,
    pub notification: Option<&'a Value>,
    pub expires_at: Option<DateTime<Utc>>,
    pub throttling: Option<f64>,
    pub temporal_q: Option<&'a Value>,
    pub lang: Option<&'a str>,
    pub jsonld_context: Option<&'a str>,
    /// When true, subscription only matches locally stored entities (§5.5.13).
    pub local_only: bool,
}

#[async_trait]
pub trait SubscriptionStore: Send + Sync {
    async fn get(&self, id: &str) -> Result<Option<Subscription>, AndromedaError>;
    async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<Subscription>, i64), AndromedaError>;
    async fn delete(&self, id: &str) -> Result<bool, AndromedaError>;
    async fn exists(&self, id: &str) -> Result<bool, AndromedaError>;
    async fn list_active(&self) -> Result<Vec<Subscription>, AndromedaError>;
    async fn claim_notification_slot(&self, id: &str, throttling_secs: Option<f64>) -> Result<bool, AndromedaError>;
    async fn mark_success(&self, id: &str) -> Result<(), AndromedaError>;
    async fn mark_failure(&self, id: &str) -> Result<(), AndromedaError>;
    async fn expire_due(&self) -> Result<Vec<String>, AndromedaError>;
    async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, AndromedaError>;
    async fn create_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), AndromedaError>;
    async fn update_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<bool, AndromedaError>;
}

/// Generates a `SubscriptionStore` implementation for the given type using static SQL strings.
///
/// Parameters:
/// - `$type`: the struct to impl `SubscriptionStore` for
/// - `table`: SQL table name (literal string)
/// - `columns`: comma-separated column list (literal string)
/// - `has_dataset_id`: whether the table has a `dataset_id` column (bool expression)
#[macro_export]
macro_rules! impl_subscription_store {
    ($type:ty, table: $table:literal, columns: $columns:literal, has_dataset_id: $hdi:expr) => {
        #[async_trait::async_trait]
        impl $crate::repository::subscription_store::SubscriptionStore for $type {
            async fn get(&self, id: &str) -> Result<Option<$crate::domain::subscription::Subscription>, $crate::error::AndromedaError> {
                let row = sqlx::query(concat!("SELECT ", $columns, " FROM ", $table, " WHERE id = $1"))
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await?;

                match row {
                    Some(row) => Ok(Some($crate::repository::subscription_store::build_subscription_from_row(&row, $hdi)?)),
                    None => Ok(None),
                }
            }

            async fn list(
                &self,
                limit: Option<i64>,
                offset: Option<i64>,
            ) -> Result<(Vec<$crate::domain::subscription::Subscription>, i64), $crate::error::AndromedaError> {
                let limit = limit.unwrap_or(100);
                let offset = offset.unwrap_or(0);

                let count_row: (i64,) = sqlx::query_as(concat!("SELECT COUNT(*) FROM ", $table)).fetch_one(&self.pool).await?;

                let rows = sqlx::query(concat!(
                    "SELECT ",
                    $columns,
                    " FROM ",
                    $table,
                    " ORDER BY created_at ASC LIMIT $1 OFFSET $2"
                ))
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?;

                let subscriptions = rows
                    .iter()
                    .map(|row| $crate::repository::subscription_store::build_subscription_from_row(row, $hdi))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok((subscriptions, count_row.0))
            }

            async fn delete(&self, id: &str) -> Result<bool, $crate::error::AndromedaError> {
                let result = sqlx::query(concat!("DELETE FROM ", $table, " WHERE id = $1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;

                Ok(result.rows_affected() > 0)
            }

            async fn exists(&self, id: &str) -> Result<bool, $crate::error::AndromedaError> {
                let row: (bool,) = sqlx::query_as(concat!("SELECT EXISTS(SELECT 1 FROM ", $table, " WHERE id = $1)"))
                    .bind(id)
                    .fetch_one(&self.pool)
                    .await?;
                Ok(row.0)
            }

            async fn list_active(&self) -> Result<Vec<$crate::domain::subscription::Subscription>, $crate::error::AndromedaError> {
                let rows = sqlx::query(concat!("SELECT ", $columns, " FROM ", $table, " WHERE status = 'active'"))
                    .fetch_all(&self.pool)
                    .await?;

                rows.iter()
                    .map(|row| $crate::repository::subscription_store::build_subscription_from_row(row, $hdi))
                    .collect::<Result<Vec<_>, _>>()
            }

            async fn claim_notification_slot(&self, id: &str, throttling_secs: Option<f64>) -> Result<bool, $crate::error::AndromedaError> {
                let result = if let Some(throttle) = throttling_secs {
                    sqlx::query(concat!(
                        "UPDATE ",
                        $table,
                        " SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW()",
                        " WHERE id = $1 AND status = 'active'",
                        " AND (last_notification IS NULL",
                        "      OR last_notification <= NOW() - make_interval(secs => $2))"
                    ))
                    .bind(id)
                    .bind(throttle)
                    .execute(&self.pool)
                    .await?
                } else {
                    sqlx::query(concat!(
                        "UPDATE ",
                        $table,
                        " SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW()",
                        " WHERE id = $1 AND status = 'active'"
                    ))
                    .bind(id)
                    .execute(&self.pool)
                    .await?
                };

                Ok(result.rows_affected() > 0)
            }

            async fn mark_success(&self, id: &str) -> Result<(), $crate::error::AndromedaError> {
                sqlx::query(concat!("UPDATE ", $table, " SET last_success = NOW(), modified_at = NOW() WHERE id = $1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn mark_failure(&self, id: &str) -> Result<(), $crate::error::AndromedaError> {
                sqlx::query(concat!("UPDATE ", $table, " SET last_failure = NOW(), modified_at = NOW() WHERE id = $1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn expire_due(&self) -> Result<Vec<String>, $crate::error::AndromedaError> {
                let rows = sqlx::query(concat!(
                    "UPDATE ",
                    $table,
                    " SET status = 'expired', is_active = false, modified_at = NOW()",
                    " WHERE status != 'expired' AND expires_at IS NOT NULL AND expires_at <= NOW()",
                    " RETURNING id"
                ))
                .fetch_all(&self.pool)
                .await?;

                Ok(rows.iter().map(|r| sqlx::Row::get::<String, _>(r, "id")).collect())
            }

            async fn create_subscription(
                &self,
                id: &str,
                fields: &$crate::repository::subscription_store::SubscriptionFields<'_>,
                status: $crate::domain::subscription::SubscriptionStatus,
            ) -> Result<(), $crate::error::AndromedaError> {
                self.create(id, fields, status).await
            }

            async fn update_subscription(
                &self,
                id: &str,
                fields: &$crate::repository::subscription_store::SubscriptionFields<'_>,
                status: $crate::domain::subscription::SubscriptionStatus,
            ) -> Result<bool, $crate::error::AndromedaError> {
                self.update(id, fields, status).await
            }

            async fn find_due_interval_subscriptions(&self) -> Result<Vec<$crate::domain::subscription::Subscription>, $crate::error::AndromedaError> {
                let rows = sqlx::query(concat!(
                    "SELECT ",
                    $columns,
                    " FROM ",
                    $table,
                    " WHERE status = 'active'",
                    " AND time_interval IS NOT NULL",
                    " AND (",
                    "       (last_notification IS NULL",
                    "        AND created_at <= NOW() - make_interval(secs => time_interval))",
                    "       OR",
                    "       (last_notification IS NOT NULL",
                    "        AND last_notification <= NOW() - make_interval(secs => time_interval))",
                    "     )"
                ))
                .fetch_all(&self.pool)
                .await?;

                rows.iter()
                    .map(|row| $crate::repository::subscription_store::build_subscription_from_row(row, $hdi))
                    .collect::<Result<Vec<_>, _>>()
            }
        }
    };
}

/// Fields that may be present in a subscription update request.
/// Uses `Option<Option<T>>` to distinguish "not provided" (`None`) from "provided" (`Some`).
pub struct StatusUpdateFields {
    pub is_active: Option<bool>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

pub fn compute_updated_status(current_status: SubscriptionStatus, fields: &StatusUpdateFields) -> SubscriptionStatus {
    let now = Utc::now();

    // If a new expires_at is provided and already in the past, keep current status
    if let Some(Some(ea)) = fields.expires_at
        && ea <= now
    {
        return current_status;
    }

    match (fields.is_active, fields.expires_at.is_some()) {
        (Some(true), false) if current_status != SubscriptionStatus::Expired => SubscriptionStatus::Active,
        (Some(false), false) if current_status != SubscriptionStatus::Expired => SubscriptionStatus::Paused,
        (Some(true), true) => SubscriptionStatus::Active,
        (Some(false), true) => SubscriptionStatus::Paused,
        (None, true) if current_status == SubscriptionStatus::Expired => SubscriptionStatus::Active,
        _ => current_status,
    }
}

pub fn generate_subscription_id() -> Result<String, AndromedaError> {
    let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:{}", Uuid::new_v4())).build()?;
    Ok(urn.to_string())
}

/// Shared row-to-subscription builder used by both `SubscriptionRepository` and
/// `CsourceSubscriptionRepository`. The `has_dataset_id` flag controls whether the
/// `dataset_id` column is read from the row.
pub fn build_subscription_from_row(row: &sqlx::postgres::PgRow, has_dataset_id: bool) -> Result<Subscription, AndromedaError> {
    let id: String = row.get("id");
    let type_: String = row.get("type");
    let subscription_name: Option<String> = row.get("subscription_name");
    let description: Option<String> = row.get("description");
    let entities_json: Option<Value> = row.get("entities");
    let watched_attributes: Option<Vec<String>> = row.get("watched_attributes");
    let notification_trigger: Option<Vec<crate::domain::subscription::NotificationTrigger>> = row
        .get::<Option<Vec<String>>, _>("notification_trigger")
        .map(|v| v.into_iter().filter_map(|s| s.parse().ok()).collect());
    let time_interval: Option<f64> = row.get("time_interval");
    let q: Option<String> = row.get("q");
    let geo_q: Option<Value> = row.get("geo_q");
    let scope_q: Option<String> = row.get("scope_q");
    let csf: Option<String> = row.get("csf");
    let dataset_id: Option<Vec<String>> = if has_dataset_id { row.get("dataset_id") } else { None };
    let is_active: bool = row.get("is_active");
    let notification_json: Value = row.get("notification");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let throttling: Option<f64> = row.get("throttling");
    let status_str: String = row.get("status");
    let status: SubscriptionStatus = status_str.parse().map_err(|e: strum::ParseError| AndromedaError::internal(e.to_string()))?;
    let temporal_q: Option<Value> = row.get("temporal_q");
    let lang: Option<String> = row.get("lang");
    let jsonld_context: Option<String> = row.get("jsonld_context");
    let local_only: bool = row.try_get("local_only").unwrap_or(false);
    let times_sent: i32 = row.get("times_sent");
    let last_notification: Option<DateTime<Utc>> = row.get("last_notification");
    let last_success: Option<DateTime<Utc>> = row.get("last_success");
    let last_failure: Option<DateTime<Utc>> = row.get("last_failure");
    let created_at: DateTime<Utc> = row.get("created_at");
    let modified_at: DateTime<Utc> = row.get("modified_at");

    let entities = entities_json
        .map(|v| serde_json::from_value(v).map_err(|e| AndromedaError::internal(format!("Failed to parse entities: {}", e))))
        .transpose()?;

    let mut notification: NotificationParams =
        serde_json::from_value(notification_json).map_err(|e| AndromedaError::internal(format!("Failed to parse notification: {}", e)))?;

    notification.times_sent = Some(times_sent);
    notification.last_notification = last_notification;
    notification.last_success = last_success;
    notification.last_failure = last_failure;

    notification.status = match (last_success, last_failure) {
        (Some(s), Some(f)) => Some(if s >= f { NotificationStatus::Ok } else { NotificationStatus::Failed }),
        (Some(_), None) => Some(NotificationStatus::Ok),
        (None, Some(_)) => Some(NotificationStatus::Failed),
        (None, None) => None,
    };

    Ok(Subscription {
        id,
        type_,
        subscription_name,
        description,
        entities,
        watched_attributes,
        notification_trigger,
        time_interval,
        q,
        geo_q,
        scope_q,
        csf,
        dataset_id,
        is_active: Some(is_active),
        notification,
        expires_at,
        throttling,
        status,
        temporal_q,
        lang,
        jsonld_context,
        local_only: if local_only { Some(true) } else { None },
        created_at,
        modified_at,
    })
}
