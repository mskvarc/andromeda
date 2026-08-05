//! Shared subscription persistence trait and macro-generated CRUD.
//!
//! The trait defines the persistence contract shared by entity and
//! context-source subscriptions. The `impl_subscription_store!` macro
//! generates implementations backed by the `subscriptions` table.

use crate::{core::error::AndromedaError, repository::subscription_store::params::SubscriptionFields};
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Trait for subscription persistence operations shared across entity and context source subscriptions.
#[async_trait]
pub trait SubscriptionStore: Send + Sync {
    /// Retrieve a subscription by identifier.
    async fn get(&self, id: &str) -> Result<Option<Subscription>, AndromedaError>;
    /// List subscriptions with pagination, returning the items and total count.
    async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<Subscription>, i64), AndromedaError>;
    /// Delete a subscription by identifier. Returns rows affected.
    async fn delete(&self, id: &str) -> Result<u64, AndromedaError>;
    /// Check whether a subscription with the given identifier exists.
    async fn exists(&self, id: &str) -> Result<bool, AndromedaError>;
    /// List all subscriptions with active status.
    async fn list_active(&self) -> Result<Vec<Subscription>, AndromedaError>;
    /// Atomically claim a notification slot. `earliest_allowed_last_notification`,
    /// when provided, gates the claim on `last_notification IS NULL OR
    /// last_notification <= $2`. The service layer computes this timestamp from
    /// the subscription's throttling configuration.
    async fn claim_notification_slot(&self, id: &str, earliest_allowed_last_notification: Option<DateTime<Utc>>) -> Result<bool, AndromedaError>;
    /// Record a successful notification delivery.
    async fn mark_success(&self, id: &str) -> Result<(), AndromedaError>;
    /// Record a failed notification delivery.
    async fn mark_failure(&self, id: &str) -> Result<(), AndromedaError>;
    /// Expire subscriptions whose `expires_at` has passed. Returns the IDs of expired subscriptions.
    async fn expire_due(&self) -> Result<Vec<String>, AndromedaError>;
    /// Find active interval-based subscriptions that are due for their next notification.
    async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, AndromedaError>;
    /// Create a new subscription with the given fields and status.
    async fn create_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), AndromedaError>;
    /// Update an existing subscription. Returns rows affected.
    async fn update_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<u64, AndromedaError>;
}

/// Generates a `SubscriptionStore` implementation backed by the `subscriptions` table.
///
/// Parameters:
/// - `$type`: the struct to impl `SubscriptionStore` for
/// - `kind`: discriminator value (`"entity"` or `"csource"`)
/// - `columns`: comma-separated column list (literal string)
/// - `has_dataset_id`: `DatasetIdPresence` constant
#[macro_export]
macro_rules! impl_subscription_store {
    ($type:ty, kind: $kind:literal, columns: $columns:literal, has_dataset_id: $hdi:expr) => {
        #[async_trait::async_trait]
        impl $crate::repository::subscription_store::repo::SubscriptionStore for $type {
            async fn get(&self, id: &str) -> Result<Option<andromeda_ngsi_ld_model::subscription::Subscription>, $crate::core::error::AndromedaError> {
                let pg_row = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await?;

                match pg_row {
                    Some(pg) => {
                        let row = $crate::repository::subscription_store::mapper::row_from_pg(&pg, $hdi)?;
                        Ok(Some($crate::repository::subscription_store::mapper::row_to_subscription(row)?))
                    }
                    None => Ok(None),
                }
            }

            async fn list(
                &self,
                limit: Option<i64>,
                offset: Option<i64>,
            ) -> Result<(Vec<andromeda_ngsi_ld_model::subscription::Subscription>, i64), $crate::core::error::AndromedaError> {
                let limit = limit.unwrap_or(100);
                let offset = offset.unwrap_or(0);

                let count_row: (i64,) = sqlx::query_as(concat!("SELECT COUNT(*) FROM subscriptions WHERE kind = '", $kind, "'"))
                    .fetch_one(&self.pool)
                    .await?;

                let pg_rows = sqlx::query(concat!(
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

                let subscriptions = pg_rows
                    .iter()
                    .map(|pg| {
                        let row = $crate::repository::subscription_store::mapper::row_from_pg(pg, $hdi)?;
                        $crate::repository::subscription_store::mapper::row_to_subscription(row)
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                Ok((subscriptions, count_row.0))
            }

            async fn delete(&self, id: &str) -> Result<u64, $crate::core::error::AndromedaError> {
                let result = sqlx::query(concat!("DELETE FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;

                Ok(result.rows_affected())
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

            async fn list_active(&self) -> Result<Vec<andromeda_ngsi_ld_model::subscription::Subscription>, $crate::core::error::AndromedaError> {
                let pg_rows = sqlx::query(concat!(
                    "SELECT ",
                    $columns,
                    " FROM subscriptions WHERE status = 'active' AND kind = '",
                    $kind,
                    "'"
                ))
                .fetch_all(&self.pool)
                .await?;

                pg_rows
                    .iter()
                    .map(|pg| {
                        let row = $crate::repository::subscription_store::mapper::row_from_pg(pg, $hdi)?;
                        $crate::repository::subscription_store::mapper::row_to_subscription(row)
                    })
                    .collect::<Result<Vec<_>, _>>()
            }

            async fn claim_notification_slot(
                &self,
                id: &str,
                earliest_allowed_last_notification: Option<chrono::DateTime<chrono::Utc>>,
            ) -> Result<bool, $crate::core::error::AndromedaError> {
                let result = if let Some(ceiling) = earliest_allowed_last_notification {
                    sqlx::query(concat!(
                        "UPDATE subscriptions",
                        " SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW()",
                        " WHERE id = $1 AND status = 'active' AND kind = '",
                        $kind,
                        "'",
                        " AND (last_notification IS NULL OR last_notification <= $2)"
                    ))
                    .bind(id)
                    .bind(ceiling)
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
                fields: &$crate::repository::subscription_store::params::SubscriptionFields<'_>,
                status: andromeda_ngsi_ld_model::subscription::SubscriptionStatus,
            ) -> Result<(), $crate::core::error::AndromedaError> {
                self.create(id, fields, status).await
            }

            async fn update_subscription(
                &self,
                id: &str,
                fields: &$crate::repository::subscription_store::params::SubscriptionFields<'_>,
                status: andromeda_ngsi_ld_model::subscription::SubscriptionStatus,
            ) -> Result<u64, $crate::core::error::AndromedaError> {
                self.update(id, fields, status).await
            }

            async fn find_due_interval_subscriptions(
                &self,
            ) -> Result<Vec<andromeda_ngsi_ld_model::subscription::Subscription>, $crate::core::error::AndromedaError> {
                let pg_rows = sqlx::query(concat!(
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

                pg_rows
                    .iter()
                    .map(|pg| {
                        let row = $crate::repository::subscription_store::mapper::row_from_pg(pg, $hdi)?;
                        $crate::repository::subscription_store::mapper::row_to_subscription(row)
                    })
                    .collect::<Result<Vec<_>, _>>()
            }
        }
    };
}
