//! Subscription store trait implementations for both entity and context-source
//! subscriptions.
//!
//! The `SubscriptionStore` trait, the `SubscriptionFields` type it references,
//! and the `create` / `update` SQL belong here because they depend on
//! binary-side service types. The repository struct definitions and the shared
//! mapper live in the context-store crate.

use crate::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    subscription::repo::SubscriptionRepository,
    subscription_store::{
        mapper::{row_from_pg, row_to_subscription, rows_to_subscriptions, stored_subscription_uri},
        params::DatasetIdPresence,
    },
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_subscription::{
    error::SubscriptionError,
    store::{SubscriptionFields, SubscriptionStore},
};
use sqlx::{Row, postgres::PgArguments, query::Query, types::Json};
use std::fmt::Display;
use url::Url;

/// Flatten an adapter failure at the policy boundary.
fn persistence_error(error: impl Display) -> SubscriptionError {
    SubscriptionError::persistence(error.to_string())
}

/// Preserve a uniqueness race as the semantic identifier conflict.
fn create_error(error: sqlx::Error, identifier: &Uri) -> SubscriptionError {
    match error {
        sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505") => SubscriptionError::already_exists(identifier),
        other => persistence_error(other),
    }
}

/// Generates a `SubscriptionStore` implementation backed by the `subscriptions` table.
macro_rules! impl_subscription_store {
    ($type:ty, kind: $kind:literal, columns: $columns:literal, has_dataset_id: $hdi:expr, create_sql: $create:expr, update_sql: $update:expr, create_binds: $create_fn:expr, update_binds: $update_fn:expr) => {
        #[async_trait::async_trait]
        impl SubscriptionStore for $type {
            async fn get(&self, id: &Uri) -> Result<Option<Subscription>, SubscriptionError> {
                let pg_row = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id.as_str())
                    .fetch_optional(&self.pool)
                    .await.map_err(persistence_error)?;
                match pg_row {
                    Some(pg) => {
                        let row = row_from_pg(&pg, $hdi).map_err(persistence_error)?;
                        Ok(Some(row_to_subscription(row).map_err(persistence_error)?))
                    }
                    None => Ok(None),
                }
            }

            async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
                let limit = limit.unwrap_or(100);
                let offset = offset.unwrap_or(0);
                let count_row: (i64,) = sqlx::query_as(concat!("SELECT COUNT(*) FROM subscriptions WHERE kind = '", $kind, "'"))
                    .fetch_one(&self.pool).await.map_err(persistence_error)?;
                let pg_rows = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE kind = '", $kind, "' ORDER BY created_at ASC LIMIT $1 OFFSET $2"))
                    .bind(limit).bind(offset).fetch_all(&self.pool).await.map_err(persistence_error)?;
                let subscriptions = rows_to_subscriptions(&pg_rows, $hdi).map_err(persistence_error)?;
                Ok((subscriptions, count_row.0))
            }

            async fn delete(&self, id: &Uri) -> Result<u64, SubscriptionError> {
                let result = sqlx::query(concat!("DELETE FROM subscriptions WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id.as_str()).execute(&self.pool).await.map_err(persistence_error)?;
                Ok(result.rows_affected())
            }

            async fn exists(&self, id: &Uri) -> Result<bool, SubscriptionError> {
                let row: (bool,) = sqlx::query_as(concat!("SELECT EXISTS(SELECT 1 FROM subscriptions WHERE id = $1 AND kind = '", $kind, "')"))
                    .bind(id.as_str()).fetch_one(&self.pool).await.map_err(persistence_error)?;
                Ok(row.0)
            }

            async fn list_active(&self) -> Result<Vec<Subscription>, SubscriptionError> {
                let pg_rows = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE status = 'active' AND kind = '", $kind, "'"))
                    .fetch_all(&self.pool).await.map_err(persistence_error)?;
                rows_to_subscriptions(&pg_rows, $hdi).map_err(persistence_error)
            }

            async fn claim_notification_slot(&self, id: &Uri, earliest_allowed_last_notification: Option<chrono::DateTime<chrono::Utc>>) -> Result<bool, SubscriptionError> {
                let result = if let Some(ceiling) = earliest_allowed_last_notification {
                    sqlx::query(concat!("UPDATE subscriptions SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW() WHERE id = $1 AND status = 'active' AND kind = '", $kind, "' AND (last_notification IS NULL OR last_notification <= $2)"))
                        .bind(id.as_str()).bind(ceiling).execute(&self.pool).await.map_err(persistence_error)?
                } else {
                    sqlx::query(concat!("UPDATE subscriptions SET times_sent = times_sent + 1, last_notification = NOW(), modified_at = NOW() WHERE id = $1 AND status = 'active' AND kind = '", $kind, "'"))
                        .bind(id.as_str()).execute(&self.pool).await.map_err(persistence_error)?
                };
                Ok(result.rows_affected() > 0)
            }

            async fn mark_success(&self, id: &Uri) -> Result<(), SubscriptionError> {
                sqlx::query(concat!("UPDATE subscriptions SET last_success = NOW(), modified_at = NOW() WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id.as_str()).execute(&self.pool).await.map_err(persistence_error)?;
                Ok(())
            }

            async fn mark_failure(&self, id: &Uri) -> Result<(), SubscriptionError> {
                sqlx::query(concat!("UPDATE subscriptions SET last_failure = NOW(), modified_at = NOW() WHERE id = $1 AND kind = '", $kind, "'"))
                    .bind(id.as_str()).execute(&self.pool).await.map_err(persistence_error)?;
                Ok(())
            }

            async fn expire_due(&self) -> Result<Vec<Uri>, SubscriptionError> {
                let rows = sqlx::query(concat!("UPDATE subscriptions SET status = 'expired', is_active = false, modified_at = NOW() WHERE status != 'expired' AND expires_at IS NOT NULL AND expires_at <= NOW() AND kind = '", $kind, "' RETURNING id"))
                    .fetch_all(&self.pool).await.map_err(persistence_error)?;
                rows.iter()
                    .map(|row| {
                        row.try_get::<String, _>("id")
                            .map_err(persistence_error)
                            .and_then(|raw| stored_subscription_uri(raw).map_err(persistence_error))
                    })
                    .collect()
            }

            async fn create_subscription(&self, id: &Uri, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), SubscriptionError> {
                let query = sqlx::query($create);
                let query = ($create_fn)(query, id, fields, status);
                query.execute(&self.pool).await.map_err(|error| create_error(error, id))?;
                tracing::info!(concat!("Created ", $kind, " subscription: {}"), id);
                Ok(())
            }

            async fn update_subscription(&self, id: &Uri, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<u64, SubscriptionError> {
                let query = sqlx::query($update);
                let query = ($update_fn)(query, id, fields, status);
                let result = query.execute(&self.pool).await.map_err(persistence_error)?;
                Ok(result.rows_affected())
            }

            async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, SubscriptionError> {
                let pg_rows = sqlx::query(concat!("SELECT ", $columns, " FROM subscriptions WHERE status = 'active' AND kind = '", $kind, "' AND time_interval IS NOT NULL AND ((last_notification IS NULL AND created_at <= NOW() - make_interval(secs => time_interval)) OR (last_notification IS NOT NULL AND last_notification <= NOW() - make_interval(secs => time_interval)))"))
                    .fetch_all(&self.pool).await.map_err(persistence_error)?;
                rows_to_subscriptions(&pg_rows, $hdi).map_err(persistence_error)
            }
        }
    };
}

/// Bind an entity-subscription create command in statement order.
fn bind_entity_create<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(|name| name.as_str()).collect::<Vec<_>>()),
        )
        .bind(
            fields
                .notification_trigger
                .map(|triggers| triggers.iter().map(ToString::to_string).collect::<Vec<_>>()),
        )
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.dataset_id.map(|ids| ids.iter().map(Uri::as_str).collect::<Vec<_>>()))
        .bind(fields.is_active)
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.temporal_q.map(Json))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(fields.local_only)
}

/// Bind an entity-subscription update command in statement order.
fn bind_entity_update<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(|name| name.as_str()).collect::<Vec<_>>()),
        )
        .bind(
            fields
                .notification_trigger
                .map(|triggers| triggers.iter().map(ToString::to_string).collect::<Vec<_>>()),
        )
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.dataset_id.map(|ids| ids.iter().map(Uri::as_str).collect::<Vec<_>>()))
        .bind(fields.is_active)
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(fields.local_only)
}

/// Bind a context-source-subscription create command in statement order.
fn bind_csource_create<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(|name| name.as_str()).collect::<Vec<_>>()),
        )
        .bind(
            fields
                .notification_trigger
                .map(|triggers| triggers.iter().map(ToString::to_string).collect::<Vec<_>>()),
        )
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.is_active)
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.temporal_q.map(Json))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
}

/// Bind a context-source-subscription update command in statement order.
fn bind_csource_update<'q>(
    query: Query<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> Query<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(|name| name.as_str()).collect::<Vec<_>>()),
        )
        .bind(
            fields
                .notification_trigger
                .map(|triggers| triggers.iter().map(ToString::to_string).collect::<Vec<_>>()),
        )
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.is_active)
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
}

/// Entity-subscription insertion statement.
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

/// Entity-subscription replacement statement.
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

/// Context-source-subscription insertion statement.
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

/// Context-source-subscription replacement statement.
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

#[cfg(test)]
mod tests {
    use crate::{csource_subscription::repo::CsourceSubscriptionRepository, subscription::repo::SubscriptionRepository};
    use andromeda_subscription::store::SubscriptionStore;

    fn assert_subscription_store<T: SubscriptionStore>() {}

    #[test]
    fn entity_subscription_repository_implements_the_policy_port_directly() {
        assert_subscription_store::<SubscriptionRepository>();
    }

    #[test]
    fn csource_subscription_repository_implements_the_policy_port_directly() {
        assert_subscription_store::<CsourceSubscriptionRepository>();
    }
}
