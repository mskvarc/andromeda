//! Shared subscription persistence trait and helpers.
//!
//! Defines the `SubscriptionStore` trait, the `impl_subscription_store!` macro
//! that generates implementations, and helper functions for status computation
//! and row deserialization shared by entity and context source subscriptions.

use crate::{
    core::error::AndromedaError,
    domain::subscription::{NotificationParams, NotificationStatus, Subscription, SubscriptionStatus},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;

/// Whether the subscription table includes a `dataset_id` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetIdPresence {
    /// The `dataset_id` column exists and should be read.
    Present,
    /// The `dataset_id` column does not exist in this table.
    Absent,
}

/// Borrowed field values used when creating or updating a subscription.
#[derive(Debug)]
pub struct SubscriptionFields<'a> {
    /// Human-readable name of the subscription.
    pub subscription_name: Option<&'a str>,
    /// Free-text description of the subscription.
    pub description: Option<&'a str>,
    /// Entity selector information as a JSON value.
    pub entities: Option<&'a Value>,
    /// Attribute IRIs that trigger notifications when changed.
    pub watched_attributes: Option<&'a [String]>,
    /// Events that trigger a notification (e.g., entityCreated).
    pub notification_trigger: Option<&'a [String]>,
    /// Periodic notification interval in seconds.
    pub time_interval: Option<f64>,
    /// NGSI-LD query expression for filtering entities.
    pub q: Option<&'a str>,
    /// Geo-spatial query as a JSON value.
    pub geo_q: Option<&'a Value>,
    /// Scope query expression.
    pub scope_q: Option<&'a str>,
    /// Context source filter expression.
    pub csf: Option<&'a str>,
    /// Dataset identifiers to restrict matching.
    pub dataset_id: Option<&'a [String]>,
    /// Whether the subscription is currently active.
    pub is_active: bool,
    /// Notification endpoint and format configuration as a JSON value.
    pub notification: Option<&'a Value>,
    /// Timestamp after which the subscription expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// Minimum interval in seconds between consecutive notifications.
    pub throttling: Option<f64>,
    /// Temporal query specification as a JSON value.
    pub temporal_q: Option<&'a Value>,
    /// Preferred language for notifications.
    pub lang: Option<&'a str>,
    /// JSON-LD context URI associated with the subscription.
    pub jsonld_context: Option<&'a str>,
    /// When true, subscription only matches locally stored entities (§5.5.13).
    pub local_only: bool,
}

/// Trait for subscription persistence operations shared across entity and context source subscriptions.
#[async_trait]
pub trait SubscriptionStore: Send + Sync {
    /// Retrieve a subscription by identifier.
    async fn get(&self, id: &str) -> Result<Option<Subscription>, AndromedaError>;
    /// List subscriptions with pagination, returning the items and total count.
    async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<Subscription>, i64), AndromedaError>;
    /// Delete a subscription by identifier. Returns `true` if a row was removed.
    async fn delete(&self, id: &str) -> Result<bool, AndromedaError>;
    /// Check whether a subscription with the given identifier exists.
    async fn exists(&self, id: &str) -> Result<bool, AndromedaError>;
    /// List all subscriptions with active status.
    async fn list_active(&self) -> Result<Vec<Subscription>, AndromedaError>;
    /// Atomically claim a notification slot, respecting throttling. Returns `true` if claimed.
    async fn claim_notification_slot(&self, id: &str, throttling_secs: Option<f64>) -> Result<bool, AndromedaError>;
    /// Record a successful notification delivery for the given subscription.
    async fn mark_success(&self, id: &str) -> Result<(), AndromedaError>;
    /// Record a failed notification delivery for the given subscription.
    async fn mark_failure(&self, id: &str) -> Result<(), AndromedaError>;
    /// Expire subscriptions whose `expires_at` has passed. Returns the IDs of expired subscriptions.
    async fn expire_due(&self) -> Result<Vec<String>, AndromedaError>;
    /// Find active interval-based subscriptions that are due for their next notification.
    async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, AndromedaError>;
    /// Create a new subscription with the given fields and status.
    async fn create_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), AndromedaError>;
    /// Update an existing subscription. Returns `true` if a row was modified.
    async fn update_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<bool, AndromedaError>;
}

/// Generates a `SubscriptionStore` implementation for the given type using static SQL strings.
///
/// All subscription types share the `subscriptions` table, discriminated by the `kind` column.
///
/// Parameters:
/// - `$type`: the struct to impl `SubscriptionStore` for
/// - `kind`: discriminator value ("entity" or "csource")
/// - `columns`: comma-separated column list (literal string)
/// - `has_dataset_id`: whether to read the `dataset_id` column from rows (`DatasetIdPresence`)
#[macro_export]
macro_rules! impl_subscription_store {
    ($type:ty, kind: $kind:literal, columns: $columns:literal, has_dataset_id: $hdi:expr) => {
        #[async_trait::async_trait]
        impl $crate::repository::subscription_store::SubscriptionStore for $type {
            async fn get(&self, id: &str) -> Result<Option<$crate::domain::subscription::Subscription>, $crate::core::error::AndromedaError> {
                let row = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
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
            ) -> Result<(Vec<$crate::domain::subscription::Subscription>, i64), $crate::core::error::AndromedaError> {
                let limit = limit.unwrap_or(100);
                let offset = offset.unwrap_or(0);

                let count_row: (i64,) = sqlx::query_as(concat!("SELECT COUNT(*) FROM subscriptions WHERE kind = '", $kind, "'"))
                    .fetch_one(&self.pool)
                    .await?;

                let rows = sqlx::query(concat!(
                    "SELECT ",
                    $columns,
                    " FROM subscriptions",
                    " WHERE kind = '",
                    $kind,
                    "'",
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

            async fn delete(&self, id: &str) -> Result<bool, $crate::core::error::AndromedaError> {
                let result = sqlx::query(concat!("DELETE FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;

                Ok(result.rows_affected() > 0)
            }

            async fn exists(&self, id: &str) -> Result<bool, $crate::core::error::AndromedaError> {
                let row: (bool,) = sqlx::query_as(concat!(
                    "SELECT EXISTS(SELECT 1 FROM subscriptions WHERE id = $1 AND kind = '",
                    $kind,
                    "')"
                ))
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.0)
            }

            async fn list_active(&self) -> Result<Vec<$crate::domain::subscription::Subscription>, $crate::core::error::AndromedaError> {
                let rows = sqlx::query(concat!(
                    "SELECT ",
                    $columns,
                    " FROM subscriptions WHERE status = 'active' AND kind = '",
                    $kind,
                    "'"
                ))
                .fetch_all(&self.pool)
                .await?;

                rows.iter()
                    .map(|row| $crate::repository::subscription_store::build_subscription_from_row(row, $hdi))
                    .collect::<Result<Vec<_>, _>>()
            }

            async fn claim_notification_slot(&self, id: &str, throttling_secs: Option<f64>) -> Result<bool, $crate::core::error::AndromedaError> {
                let result = if let Some(throttle) = throttling_secs {
                    sqlx::query(concat!(
                        "UPDATE subscriptions",
                        " SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW()",
                        " WHERE id = $1 AND status = 'active' AND kind = '",
                        $kind,
                        "'",
                        " AND (last_notification IS NULL",
                        "      OR last_notification <= NOW() - make_interval(secs => $2))"
                    ))
                    .bind(id)
                    .bind(throttle)
                    .execute(&self.pool)
                    .await?
                } else {
                    sqlx::query(concat!(
                        "UPDATE subscriptions",
                        " SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW()",
                        " WHERE id = $1 AND status = 'active' AND kind = '",
                        $kind,
                        "'"
                    ))
                    .bind(id)
                    .execute(&self.pool)
                    .await?
                };

                Ok(result.rows_affected() > 0)
            }

            async fn mark_success(&self, id: &str) -> Result<(), $crate::core::error::AndromedaError> {
                sqlx::query(concat!(
                    "UPDATE subscriptions SET last_success = NOW(), modified_at = NOW() WHERE id = $1 AND kind = '",
                    $kind,
                    "'"
                ))
                .bind(id)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn mark_failure(&self, id: &str) -> Result<(), $crate::core::error::AndromedaError> {
                sqlx::query(concat!(
                    "UPDATE subscriptions SET last_failure = NOW(), modified_at = NOW() WHERE id = $1 AND kind = '",
                    $kind,
                    "'"
                ))
                .bind(id)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn expire_due(&self) -> Result<Vec<String>, $crate::core::error::AndromedaError> {
                let rows = sqlx::query(concat!(
                    "UPDATE subscriptions",
                    " SET status = 'expired', is_active = false, modified_at = NOW()",
                    " WHERE status != 'expired' AND expires_at IS NOT NULL AND expires_at <= NOW()",
                    " AND kind = '",
                    $kind,
                    "'",
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
            ) -> Result<(), $crate::core::error::AndromedaError> {
                self.create(id, fields, status).await
            }

            async fn update_subscription(
                &self,
                id: &str,
                fields: &$crate::repository::subscription_store::SubscriptionFields<'_>,
                status: $crate::domain::subscription::SubscriptionStatus,
            ) -> Result<bool, $crate::core::error::AndromedaError> {
                self.update(id, fields, status).await
            }

            async fn find_due_interval_subscriptions(&self) -> Result<Vec<$crate::domain::subscription::Subscription>, $crate::core::error::AndromedaError> {
                let rows = sqlx::query(concat!(
                    "SELECT ",
                    $columns,
                    " FROM subscriptions",
                    " WHERE status = 'active' AND kind = '",
                    $kind,
                    "'",
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
#[derive(Debug)]
pub struct StatusUpdateFields {
    /// New active/paused flag, if provided in the update request.
    pub is_active: Option<bool>,
    /// New expiration timestamp; outer `None` means not provided, inner `None` means cleared.
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

/// Derive the new subscription status based on the current status and the update fields provided.
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
        (Some(true), false) | (Some(false), false) => current_status,
        (None, true) | (None, false) => current_status,
    }
}

/// Shared row-to-subscription builder used by both `SubscriptionRepository` and
/// `CsourceSubscriptionRepository`. The `dataset_id_presence` parameter controls whether the
/// `dataset_id` column is read from the row.
pub fn build_subscription_from_row(row: &sqlx::postgres::PgRow, dataset_id_presence: DatasetIdPresence) -> Result<Subscription, AndromedaError> {
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
    let dataset_id: Option<Vec<String>> = match dataset_id_presence {
        DatasetIdPresence::Present => row.get("dataset_id"),
        DatasetIdPresence::Absent => None,
    };
    let is_active: bool = row.get("is_active");
    let notification_json: Value = row.get("notification");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let throttling: Option<f64> = row.get("throttling");
    let status: SubscriptionStatus = row.get("status");
    let temporal_q: Option<Value> = row.get("temporal_q");
    let lang: Option<String> = row.get("lang");
    let jsonld_context: Option<String> = row.get("jsonld_context");
    let local_only: bool = match row.try_get("local_only") {
        Ok(v) => v,
        Err(sqlx::Error::ColumnNotFound(_)) | Err(sqlx::Error::ColumnDecode { .. }) => false,
        Err(e) => return Err(AndromedaError::internal(format!("Failed to read local_only column: {}", e))),
    };
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

#[cfg(test)]
mod tests {
    use super::{StatusUpdateFields, compute_updated_status};
    use crate::domain::subscription::SubscriptionStatus;
    use chrono::{Duration, Utc};

    #[test]
    fn active_with_is_active_true_stays_active() {
        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: Some(true),
                expires_at: None,
            },
        );
        assert_eq!(status, SubscriptionStatus::Active);
    }

    #[test]
    fn active_with_is_active_false_becomes_paused() {
        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: Some(false),
                expires_at: None,
            },
        );
        assert_eq!(status, SubscriptionStatus::Paused);
    }

    #[test]
    fn expired_with_is_active_true_no_expiry_change_stays_expired() {
        let status = compute_updated_status(
            SubscriptionStatus::Expired,
            &StatusUpdateFields {
                is_active: Some(true),
                expires_at: None,
            },
        );
        assert_eq!(status, SubscriptionStatus::Expired);
    }

    #[test]
    fn expired_with_new_future_expires_at_reactivates() {
        let future = Utc::now() + Duration::hours(1);
        let status = compute_updated_status(
            SubscriptionStatus::Expired,
            &StatusUpdateFields {
                is_active: None,
                expires_at: Some(Some(future)),
            },
        );
        assert_eq!(status, SubscriptionStatus::Active);
    }

    #[test]
    fn expired_with_past_expires_at_stays_expired() {
        let past = Utc::now() - Duration::hours(1);
        let status = compute_updated_status(
            SubscriptionStatus::Expired,
            &StatusUpdateFields {
                is_active: None,
                expires_at: Some(Some(past)),
            },
        );
        assert_eq!(status, SubscriptionStatus::Expired);
    }

    #[test]
    fn active_with_no_changes_stays_active() {
        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: None,
                expires_at: None,
            },
        );
        assert_eq!(status, SubscriptionStatus::Active);
    }

    #[test]
    fn paused_with_is_active_true_and_new_expiry_becomes_active() {
        let future = Utc::now() + Duration::hours(1);
        let status = compute_updated_status(
            SubscriptionStatus::Paused,
            &StatusUpdateFields {
                is_active: Some(true),
                expires_at: Some(Some(future)),
            },
        );
        assert_eq!(status, SubscriptionStatus::Active);
    }
}
