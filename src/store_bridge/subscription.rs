//! Subscription store trait implementations for both entity and context-source
//! subscriptions.
//!
//! The `SubscriptionStore` trait, the `SubscriptionFields` type it references,
//! and the `create` / `update` SQL belong here because they depend on
//! binary-side service types. The repository struct definitions and the shared
//! mapper live in the context-store crate.

use crate::{
    error::AndromedaError,
    service::subscription::store::{SubscriptionFields, SubscriptionStore},
};
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_postgresql_context_store::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    subscription::repo::SubscriptionRepository,
    subscription_store::{
        mapper::{row_from_pg, row_to_subscription, rows_to_subscriptions},
        params::DatasetIdPresence,
    },
};
use sqlx::{Row, postgres::PgArguments, query::Query};

/// Generates a `SubscriptionStore` implementation backed by the `subscriptions` table.
macro_rules! impl_subscription_store {
    ($type:ty, kind: $kind:literal, columns: $columns:literal, has_dataset_id: $hdi:expr, create_sql: $create:expr, update_sql: $update:expr, create_binds: $create_fn:expr, update_binds: $update_fn:expr) => {
        #[async_trait::async_trait]
        impl SubscriptionStore for $type {
            async fn get(&self, id: &str) -> Result<Option<Subscription>, AndromedaError> {
                let pg_row = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await?;
                match pg_row {
                    Some(pg) => {
                        let row = row_from_pg(&pg, $hdi)?;
                        Ok(Some(row_to_subscription(row)?))
                    }
                    None => Ok(None),
                }
            }

            async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<Subscription>, i64), AndromedaError> {
                let limit = limit.unwrap_or(100);
                let offset = offset.unwrap_or(0);
                let count_row: (i64,) = sqlx::query_as(concat!("SELECT COUNT(*) FROM subscriptions WHERE kind = '", $kind, "'"))
                    .fetch_one(&self.pool).await?;
                let pg_rows = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE kind = '", $kind, "' ORDER BY created_at ASC LIMIT $1 OFFSET $2"))
                    .bind(limit).bind(offset).fetch_all(&self.pool).await?;
                let subscriptions = rows_to_subscriptions(&pg_rows, $hdi).map_err(AndromedaError::from)?;
                Ok((subscriptions, count_row.0))
            }

            async fn delete(&self, id: &str) -> Result<u64, AndromedaError> {
                let result = sqlx::query(concat!("DELETE FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id).execute(&self.pool).await?;
                Ok(result.rows_affected())
            }

            async fn exists(&self, id: &str) -> Result<bool, AndromedaError> {
                let row: (bool,) = sqlx::query_as(concat!("SELECT EXISTS(SELECT 1 FROM subscriptions WHERE id = $1 AND kind = '", $kind, "')"))
                    .bind(id).fetch_one(&self.pool).await?;
                Ok(row.0)
            }

            async fn list_active(&self) -> Result<Vec<Subscription>, AndromedaError> {
                let pg_rows = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE status = 'active' AND kind = '", $kind, "'"))
                    .fetch_all(&self.pool).await?;
                Ok(rows_to_subscriptions(&pg_rows, $hdi)?)
            }

            async fn claim_notification_slot(&self, id: &str, earliest_allowed_last_notification: Option<chrono::DateTime<chrono::Utc>>) -> Result<bool, AndromedaError> {
                let result = if let Some(ceiling) = earliest_allowed_last_notification {
                    sqlx::query(concat!("UPDATE subscriptions SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW() WHERE id = $1 AND status = 'active' AND kind = '", $kind, "' AND (last_notification IS NULL OR last_notification <= $2)"))
                        .bind(id).bind(ceiling).execute(&self.pool).await?
                } else {
                    sqlx::query(concat!("UPDATE subscriptions SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW() WHERE id = $1 AND status = 'active' AND kind = '", $kind, "'"))
                        .bind(id).execute(&self.pool).await?
                };
                Ok(result.rows_affected() > 0)
            }

            async fn mark_success(&self, id: &str) -> Result<(), AndromedaError> {
                sqlx::query(concat!("UPDATE subscriptions SET last_success = NOW(), modified_at = NOW() WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id).execute(&self.pool).await?;
                Ok(())
            }

            async fn mark_failure(&self, id: &str) -> Result<(), AndromedaError> {
                sqlx::query(concat!("UPDATE subscriptions SET last_failure = NOW(), modified_at = NOW() WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id).execute(&self.pool).await?;
                Ok(())
            }

            async fn expire_due(&self) -> Result<Vec<String>, AndromedaError> {
                let rows = sqlx::query(concat!("UPDATE subscriptions SET status = 'expired', is_active = false, modified_at = NOW() WHERE status != 'expired' AND expires_at IS NOT NULL AND expires_at <= NOW() AND kind = '", $kind, "' RETURNING id"))
                    .fetch_all(&self.pool).await?;
                Ok(rows.iter().map(|r| Row::get::<String, _>(r, "id")).collect())
            }

            async fn create_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), AndromedaError> {
                let query = sqlx::query($create);
                let query = ($create_fn)(query, id, fields, status);
                query.execute(&self.pool).await?;
                tracing::info!(concat!("Created ", $kind, " subscription: {}"), id);
                Ok(())
            }

            async fn update_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<u64, AndromedaError> {
                let query = sqlx::query($update);
                let query = ($update_fn)(query, id, fields, status);
                let result = query.execute(&self.pool).await?;
                Ok(result.rows_affected())
            }

            async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, AndromedaError> {
                let pg_rows = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE status = 'active' AND kind = '", $kind, "' AND time_interval IS NOT NULL AND ((last_notification IS NULL AND created_at <= NOW() - make_interval(secs => time_interval)) OR (last_notification IS NOT NULL AND last_notification <= NOW() - make_interval(secs => time_interval)))"))
                    .fetch_all(&self.pool).await?;
                Ok(rows_to_subscriptions(&pg_rows, $hdi)?)
            }
        }
    };
}

fn bind_entity_create<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q str,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id)
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.cloned())
        .bind(fields.watched_attributes)
        .bind(fields.notification_trigger)
        .bind(fields.time_interval)
        .bind(fields.q)
        .bind(fields.geo_q.cloned())
        .bind(fields.scope_q)
        .bind(fields.csf)
        .bind(fields.dataset_id)
        .bind(fields.is_active)
        .bind(fields.notification.cloned())
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.temporal_q.cloned())
        .bind(fields.lang)
        .bind(fields.jsonld_context)
        .bind(fields.local_only)
}

fn bind_entity_update<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q str,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id)
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.cloned())
        .bind(fields.watched_attributes)
        .bind(fields.notification_trigger)
        .bind(fields.time_interval)
        .bind(fields.q)
        .bind(fields.geo_q.cloned())
        .bind(fields.scope_q)
        .bind(fields.csf)
        .bind(fields.dataset_id)
        .bind(fields.is_active)
        .bind(fields.notification.cloned())
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.lang)
        .bind(fields.jsonld_context)
        .bind(fields.local_only)
}

fn bind_csource_create<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q str,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id)
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.cloned())
        .bind(fields.watched_attributes)
        .bind(fields.notification_trigger)
        .bind(fields.time_interval)
        .bind(fields.q)
        .bind(fields.geo_q.cloned())
        .bind(fields.scope_q)
        .bind(fields.csf)
        .bind(fields.is_active)
        .bind(fields.notification.cloned())
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.temporal_q.cloned())
        .bind(fields.lang)
        .bind(fields.jsonld_context)
}

fn bind_csource_update<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q str,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id)
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.cloned())
        .bind(fields.watched_attributes)
        .bind(fields.notification_trigger)
        .bind(fields.time_interval)
        .bind(fields.q)
        .bind(fields.geo_q.cloned())
        .bind(fields.scope_q)
        .bind(fields.csf)
        .bind(fields.is_active)
        .bind(fields.notification.cloned())
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.lang)
        .bind(fields.jsonld_context)
}

const ENTITY_CREATE_SQL: &str = r#"
INSERT INTO subscriptions (
    id, kind, type, subscription_name, description, entities,
    watched_attributes, notification_trigger, time_interval,
    q, geo_q, scope_q, csf, dataset_id, is_active, notification,
    expires_at, throttling, status, temporal_q, lang,
    jsonld_context, local_only, created_at, modified_at
) VALUES (
    $1, 'entity', 'Subscription', $2, $3, $4,
    $5, $6, $7,
    $8, $9, $10, $11, $12, $13, $14,
    $15, $16, $17::subscription_status, $18, $19,
    $20, $21, NOW(), NOW()
)
"#;

const ENTITY_UPDATE_SQL: &str = r#"
UPDATE subscriptions SET
    subscription_name = $2, description = $3, entities = $4,
    watched_attributes = $5, notification_trigger = $6, time_interval = $7,
    q = $8, geo_q = $9, scope_q = $10, csf = $11, dataset_id = $12,
    is_active = $13, notification = $14, expires_at = $15, throttling = $16,
    status = $17::subscription_status, lang = $18, jsonld_context = $19,
    local_only = $20, modified_at = NOW()
WHERE id = $1 AND kind = 'entity'
"#;

const CSOURCE_CREATE_SQL: &str = r#"
INSERT INTO subscriptions (
    id, kind, type, subscription_name, description, entities,
    watched_attributes, notification_trigger, time_interval,
    q, geo_q, scope_q, csf, is_active, notification,
    expires_at, throttling, status, temporal_q, lang,
    jsonld_context, created_at, modified_at
) VALUES (
    $1, 'csource', 'Subscription', $2, $3, $4,
    $5, $6, $7,
    $8, $9, $10, $11, $12, $13,
    $14, $15, $16::subscription_status, $17, $18,
    $19, NOW(), NOW()
)
"#;

const CSOURCE_UPDATE_SQL: &str = r#"
UPDATE subscriptions SET
    subscription_name = $2, description = $3, entities = $4,
    watched_attributes = $5, notification_trigger = $6, time_interval = $7,
    q = $8, geo_q = $9, scope_q = $10, csf = $11,
    is_active = $12, notification = $13, expires_at = $14, throttling = $15,
    status = $16::subscription_status, lang = $17, jsonld_context = $18,
    modified_at = NOW()
WHERE id = $1 AND kind = 'csource'
"#;

impl_subscription_store!(
    SubscriptionRepository,
    kind: "entity",
    columns: "id, type, subscription_name, description, entities, \
              watched_attributes, notification_trigger, time_interval, \
              q, geo_q, scope_q, csf, dataset_id, is_active, notification, \
              expires_at, throttling, status::text AS status, temporal_q, lang, \
              jsonld_context, local_only, times_sent, last_notification, \
              last_success, last_failure, created_at, modified_at",
    has_dataset_id: DatasetIdPresence::Present,
    create_sql: ENTITY_CREATE_SQL,
    update_sql: ENTITY_UPDATE_SQL,
    create_binds: bind_entity_create,
    update_binds: bind_entity_update
);

impl_subscription_store!(
    CsourceSubscriptionRepository,
    kind: "csource",
    columns: "id, type, subscription_name, description, entities, \
              watched_attributes, notification_trigger, time_interval, \
              q, geo_q, scope_q, csf, is_active, notification, \
              expires_at, throttling, status::text AS status, temporal_q, lang, \
              jsonld_context, local_only, times_sent, last_notification, \
              last_success, last_failure, created_at, modified_at",
    has_dataset_id: DatasetIdPresence::Absent,
    create_sql: CSOURCE_CREATE_SQL,
    update_sql: CSOURCE_UPDATE_SQL,
    create_binds: bind_csource_create,
    update_binds: bind_csource_update
);
