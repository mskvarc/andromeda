//! Subscription store trait implementations for both entity and context-source
//! subscriptions.
//!
//! The `SubscriptionStore` trait and the `SubscriptionFields` type it
//! references belong to the policy crate; the statements a subscription's row
//! is created and replaced with live in `create_update_sql`, the delivery
//! counters and their statements in `delivery_state`. This module owns the
//! projection every read shares and the port implementation itself.

use crate::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    subscription::repo::SubscriptionRepository,
    subscription_store::{
        create_update_sql::{
            bind_csource_create,
            bind_csource_update,
            bind_entity_create,
            bind_entity_update,
            csource_create_sql,
            csource_update_sql,
            entity_create_sql,
            entity_update_sql,
        },
        delivery_state::{DeliveryOutcome, claim_statement, interval_elapsed, join_delivery_state, mark_statement, state_insert_sql},
        failure::{create_error, persistence_error},
        mapper::{row_from_pg, row_to_stored_subscription, rows_to_stored_subscriptions, stored_subscription_uri},
        params::DatasetIdPresence,
        stored_kind::{StoredKind, of_kind},
    },
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{aggregate::count_all, cast::cast, literal::text_literal, pg_type::PostgresType};
use andromeda_subscription::{
    error::SubscriptionError,
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use sea_query::{Expr, ExprTrait, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::Row;

/// The `status::text AS status` read cast: a query decodes the enum as text.
fn status_as_text() -> SimpleExpr {
    cast(Expr::col((Subscriptions::Table, Subscriptions::Status)), PostgresType::Text)
}

/// Add the stored-subscription projection to `select`, in the column set the
/// mapper reads by name; `dataset_id` only for the family that stores it. The
/// delivery counters come from the joined state row.
fn project_subscription(select: &mut SelectStatement, kind: StoredKind) {
    select
        .column((Subscriptions::Table, Subscriptions::Id))
        .column((Subscriptions::Table, Subscriptions::SubscriptionKey))
        .column((Subscriptions::Table, Subscriptions::SubscriptionName))
        .column((Subscriptions::Table, Subscriptions::Description))
        .column((Subscriptions::Table, Subscriptions::Entities))
        .column((Subscriptions::Table, Subscriptions::WatchedAttributes))
        .column((Subscriptions::Table, Subscriptions::NotificationTrigger))
        .column((Subscriptions::Table, Subscriptions::TimeInterval))
        .column((Subscriptions::Table, Subscriptions::Q))
        .column((Subscriptions::Table, Subscriptions::GeoQ))
        .column((Subscriptions::Table, Subscriptions::ScopeQ))
        .column((Subscriptions::Table, Subscriptions::Csf))
        .column((Subscriptions::Table, Subscriptions::Notification))
        .column((Subscriptions::Table, Subscriptions::ExpiresAt))
        .column((Subscriptions::Table, Subscriptions::Throttling))
        .expr_as(status_as_text(), Subscriptions::Status)
        .column((Subscriptions::Table, Subscriptions::TemporalQ))
        .column((Subscriptions::Table, Subscriptions::Lang))
        .column((Subscriptions::Table, Subscriptions::JsonldContext))
        .column((Subscriptions::Table, Subscriptions::LocalOnly))
        .column((Subscriptions::Table, Subscriptions::SplitEntities))
        .column((Subscriptions::Table, Subscriptions::CreatedAt))
        .column((Subscriptions::Table, Subscriptions::ModifiedAt));
    if kind.has_dataset_id() {
        select.column((Subscriptions::Table, Subscriptions::DatasetId));
    }
    join_delivery_state(select);
}

/// A projecting select of one family with its state row joined, ready for a
/// caller's `WHERE`.
fn projected_select(kind: StoredKind) -> SelectStatement {
    let mut select = Query::select();
    select.from(Subscriptions::Table);
    project_subscription(&mut select, kind);
    select
}

/// The `WHERE id = $id AND kind` pin of one subscription.
fn one_of(id: &Uri, kind: StoredKind) -> sea_query::Cond {
    sea_query::Cond::all()
        .add(Expr::col((Subscriptions::Table, Subscriptions::Id)).eq(id.as_str()))
        .add(of_kind(kind))
}

/// Generates a `SubscriptionStore` implementation backed by the `subscriptions`
/// table and its `subscription_delivery_state` row, its statements built
/// through sea-query.
macro_rules! impl_subscription_store {
    ($type:ty, kind: $kind:expr, has_dataset_id: $hdi:expr, create_sql: $create:path, update_sql: $update:path, create_binds: $create_fn:path, update_binds: $update_fn:path) => {
        #[async_trait::async_trait]
        impl SubscriptionStore for $type {
            async fn get(&self, id: &Uri) -> Result<Option<StoredSubscription>, SubscriptionError> {
                let mut select = projected_select($kind);
                select.cond_where(one_of(id, $kind));
                let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
                let pg_row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                match pg_row {
                    Some(pg) => {
                        let row = row_from_pg(&pg, $hdi).map_err(persistence_error)?;
                        Ok(Some(row_to_stored_subscription(row).map_err(persistence_error)?))
                    }
                    None => Ok(None),
                }
            }

            async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
                let limit = limit.unwrap_or(100);
                let offset = offset.unwrap_or(0);
                let count = Query::select()
                    .expr(count_all())
                    .from(Subscriptions::Table)
                    .and_where(of_kind($kind))
                    .to_owned();
                let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
                let count_row: (i64,) = sqlx::query_as_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(persistence_error)?;

                let mut select = projected_select($kind);
                select
                    .and_where(of_kind($kind))
                    .order_by((Subscriptions::Table, Subscriptions::CreatedAt), Order::Asc)
                    .limit(<i64 as Ord>::max(limit, 0) as u64)
                    .offset(<i64 as Ord>::max(offset, 0) as u64);
                let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
                let pg_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                // A listing renders resources, so the surrogate key each row
                // carries is dropped rather than threaded into the response.
                let subscriptions = rows_to_stored_subscriptions(&pg_rows, $hdi)
                    .map_err(persistence_error)?
                    .into_iter()
                    .map(|stored| stored.subscription)
                    .collect();
                Ok((subscriptions, count_row.0))
            }

            async fn delete(&self, id: &Uri) -> Result<u64, SubscriptionError> {
                let statement = Query::delete().from_table(Subscriptions::Table).cond_where(one_of(id, $kind)).to_owned();
                let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
                let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .execute(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                Ok(result.rows_affected())
            }

            async fn exists(&self, id: &Uri) -> Result<bool, SubscriptionError> {
                let inner = Query::select()
                    .expr(Expr::val(1))
                    .from(Subscriptions::Table)
                    .cond_where(one_of(id, $kind))
                    .to_owned();
                let statement = Query::select().expr(Expr::exists(inner)).to_owned();
                let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
                let row: (bool,) = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                Ok(row.0)
            }

            async fn list_active(&self) -> Result<Vec<StoredSubscription>, SubscriptionError> {
                let mut select = projected_select($kind);
                select
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::Status)).eq(text_literal("active")))
                    .and_where(of_kind($kind));
                let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
                let pg_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                rows_to_stored_subscriptions(&pg_rows, $hdi).map_err(persistence_error)
            }

            async fn claim_notification_slot(
                &self,
                id: &Uri,
                earliest_allowed_last_notification: Option<chrono::DateTime<chrono::Utc>>,
            ) -> Result<bool, SubscriptionError> {
                let (sql, values) = claim_statement(id, $kind, earliest_allowed_last_notification).build_sqlx(PostgresQueryBuilder);
                let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .execute(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                Ok(result.rows_affected() > 0)
            }

            async fn mark_success(&self, id: &Uri) -> Result<(), SubscriptionError> {
                let (sql, values) = mark_statement(DeliveryOutcome::Success, id, $kind).build_sqlx(PostgresQueryBuilder);
                sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .execute(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                Ok(())
            }

            async fn mark_failure(&self, id: &Uri) -> Result<(), SubscriptionError> {
                let (sql, values) = mark_statement(DeliveryOutcome::Failure, id, $kind).build_sqlx(PostgresQueryBuilder);
                sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .execute(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                Ok(())
            }

            async fn expire_due(&self, cutoff: chrono::DateTime<chrono::Utc>) -> Result<Vec<Uri>, SubscriptionError> {
                let statement = Query::update()
                    .table(Subscriptions::Table)
                    .value(Subscriptions::Status, cast(text_literal("expired"), PostgresType::SubscriptionStatus))
                    .value(Subscriptions::ModifiedAt, Expr::val(cutoff))
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::Status)).ne(text_literal("expired")))
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::ExpiresAt)).is_not_null())
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::ExpiresAt)).lte(cutoff))
                    .and_where(of_kind($kind))
                    .returning(Query::returning().column(Subscriptions::Id))
                    .to_owned();
                let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
                let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                rows.iter()
                    .map(|row| {
                        row.try_get::<String, _>("id")
                            .map_err(persistence_error)
                            .and_then(|raw| stored_subscription_uri(raw).map_err(persistence_error))
                    })
                    .collect()
            }

            /// The subscription row and its delivery-state row land in one
            /// transaction, so a subscription never exists without the row its
            /// counters live on.
            async fn create_subscription(&self, id: &Uri, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), SubscriptionError> {
                let sql = $create()?;
                let state_sql = state_insert_sql()?;
                let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
                let query = ($create_fn)(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())), id, fields, status);
                let created = query.fetch_one(&mut *transaction).await.map_err(|error| create_error(error, id))?;
                let subscription_key: i64 = created.try_get("subscription_key").map_err(persistence_error)?;
                sqlx::query(sqlx::AssertSqlSafe(state_sql.as_str()))
                    .bind(subscription_key)
                    .execute(&mut *transaction)
                    .await
                    .map_err(persistence_error)?;
                transaction.commit().await.map_err(persistence_error)?;
                tracing::info!("Created {} subscription: {}", $kind.wire(), id);
                Ok(())
            }

            async fn update_subscription(&self, id: &Uri, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<u64, SubscriptionError> {
                let sql = $update();
                let query = ($update_fn)(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())), id, fields, status);
                let result = query.execute(&self.pool).await.map_err(persistence_error)?;
                Ok(result.rows_affected())
            }

            async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, SubscriptionError> {
                let mut select = projected_select($kind);
                select
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::Status)).eq(text_literal("active")))
                    .and_where(of_kind($kind))
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::TimeInterval)).is_not_null())
                    .cond_where(interval_elapsed());
                let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
                let pg_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                // An interval notification is sent straight to the endpoint and
                // never enqueues a delivery, so it needs no surrogate key.
                Ok(rows_to_stored_subscriptions(&pg_rows, $hdi)
                    .map_err(persistence_error)?
                    .into_iter()
                    .map(|stored| stored.subscription)
                    .collect())
            }
        }
    };
}

impl_subscription_store!(
    SubscriptionRepository,
    kind: StoredKind::Entity,
    has_dataset_id: DatasetIdPresence::Present,
    create_sql: entity_create_sql,
    update_sql: entity_update_sql,
    create_binds: bind_entity_create,
    update_binds: bind_entity_update
);

impl_subscription_store!(
    CsourceSubscriptionRepository,
    kind: StoredKind::Csource,
    has_dataset_id: DatasetIdPresence::Absent,
    create_sql: csource_create_sql,
    update_sql: csource_update_sql,
    create_binds: bind_csource_create,
    update_binds: bind_csource_update
);

#[cfg(test)]
mod tests {
    use crate::{
        csource_subscription::repo::CsourceSubscriptionRepository,
        subscription::repo::SubscriptionRepository,
        subscription_store::{postgresql::projected_select, stored_kind::StoredKind},
    };
    use andromeda_subscription::store::SubscriptionStore;
    use sea_query::PostgresQueryBuilder;

    fn assert_subscription_store<T: SubscriptionStore>() {}

    #[test]
    fn entity_subscription_repository_implements_the_policy_port_directly() {
        assert_subscription_store::<SubscriptionRepository>();
    }

    #[test]
    fn csource_subscription_repository_implements_the_policy_port_directly() {
        assert_subscription_store::<CsourceSubscriptionRepository>();
    }

    #[test]
    fn every_projection_reads_the_delivery_counters_from_the_joined_state_row() {
        let sql = projected_select(StoredKind::Entity).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"LEFT JOIN "subscription_delivery_state""#), "{sql}");
        assert!(sql.contains(r#""subscriptions"."dataset_id""#), "{sql}");
        for counter in ["times_sent", "last_notification", "last_success", "last_failure"] {
            assert!(
                !sql.contains(&format!(r#""subscriptions"."{counter}""#)),
                "the subscription row carries no {counter}: {sql}"
            );
        }
        let csource = projected_select(StoredKind::Csource).to_string(PostgresQueryBuilder);
        assert!(!csource.contains("dataset_id"), "{csource}");
    }
}
