//! Subscription store trait implementations for both entity and context-source
//! subscriptions.
//!
//! The `SubscriptionStore` trait, the `SubscriptionFields` type it references,
//! and the `create` / `update` statements belong here because they depend on
//! binary-side service types. The repository struct definitions and the shared
//! mapper live in the context-store crate.

use crate::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    subscription::repo::SubscriptionRepository,
    subscription_store::{
        mapper::{row_from_pg, row_to_stored_subscription, rows_to_stored_subscriptions, stored_subscription_uri},
        params::DatasetIdPresence,
    },
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    clock::now,
    interval::make_interval_secs,
    literal::text_literal,
    pg_type::PostgresType,
    placeholder::external_placeholder,
};
use andromeda_subscription::{
    error::SubscriptionError,
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use sea_query::{Cond, Expr, ExprTrait, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, postgres::PgArguments, query::Query as SqlxQuery, types::Json};
use std::fmt::Display;
use url::Url;

/// Which subscription family a repository serves; both share the `subscriptions`
/// table and are told apart by `kind`.
#[derive(Clone, Copy)]
enum StoredKind {
    /// An entity subscription (clause 5.8), which narrows by `datasetId`.
    Entity,
    /// A context-source subscription (clause 5.11), which does not.
    Csource,
}

impl StoredKind {
    /// The `kind` token a row of this family carries.
    const fn wire(self) -> &'static str {
        match self {
            StoredKind::Entity => "entity",
            StoredKind::Csource => "csource",
        }
    }

    /// Whether this family stores a `datasetId` column.
    const fn has_dataset_id(self) -> bool {
        matches!(self, StoredKind::Entity)
    }
}

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

/// The `status::text AS status` read cast: a query decodes the enum as text.
fn status_as_text() -> SimpleExpr {
    cast(Expr::col(Subscriptions::Status), PostgresType::Text)
}

/// The predicate confining a statement to one subscription family, inline like
/// the fixed token it is rather than bound.
fn of_kind(kind: StoredKind) -> SimpleExpr {
    Expr::col(Subscriptions::Kind).eq(text_literal(kind.wire()))
}

/// Add the stored-subscription projection to `select`, in the column set the
/// mapper reads by name; `dataset_id` only for the family that stores it.
fn project_subscription(select: &mut SelectStatement, kind: StoredKind) {
    select
        .column(Subscriptions::Id)
        .column(Subscriptions::SubscriptionKey)
        .column(Subscriptions::Type)
        .column(Subscriptions::SubscriptionName)
        .column(Subscriptions::Description)
        .column(Subscriptions::Entities)
        .column(Subscriptions::WatchedAttributes)
        .column(Subscriptions::NotificationTrigger)
        .column(Subscriptions::TimeInterval)
        .column(Subscriptions::Q)
        .column(Subscriptions::GeoQ)
        .column(Subscriptions::ScopeQ)
        .column(Subscriptions::Csf)
        .column(Subscriptions::IsActive)
        .column(Subscriptions::Notification)
        .column(Subscriptions::ExpiresAt)
        .column(Subscriptions::Throttling)
        .expr_as(status_as_text(), Subscriptions::Status)
        .column(Subscriptions::TemporalQ)
        .column(Subscriptions::Lang)
        .column(Subscriptions::JsonldContext)
        .column(Subscriptions::LocalOnly)
        .column(Subscriptions::TimesSent)
        .column(Subscriptions::LastNotification)
        .column(Subscriptions::LastSuccess)
        .column(Subscriptions::LastFailure)
        .column(Subscriptions::CreatedAt)
        .column(Subscriptions::ModifiedAt);
    if kind.has_dataset_id() {
        select.column(Subscriptions::DatasetId);
    }
}

/// A projecting select of one family, ready for a caller's `WHERE`.
fn projected_select(kind: StoredKind) -> SelectStatement {
    let mut select = Query::select();
    project_subscription(&mut select, kind);
    select.from(Subscriptions::Table);
    select
}

/// `NOW() - make_interval(secs => time_interval)`: the instant a periodic
/// subscription of the row's interval last became due.
fn interval_deadline() -> SimpleExpr {
    Expr::expr(now()).sub(make_interval_secs(Expr::col(Subscriptions::TimeInterval)))
}

/// The due-interval predicate: an active periodic subscription whose interval
/// has elapsed since its last notification, or since creation if never sent.
fn interval_elapsed() -> Cond {
    let deadline = interval_deadline();
    let never_sent = Cond::all()
        .add(Expr::col(Subscriptions::LastNotification).is_null())
        .add(Expr::col(Subscriptions::CreatedAt).lte(interval_deadline()));
    let since_last = Cond::all()
        .add(Expr::col(Subscriptions::LastNotification).is_not_null())
        .add(Expr::col(Subscriptions::LastNotification).lte(deadline));
    Cond::any().add(never_sent).add(since_last)
}

/// Bind an entity-subscription create command in statement order.
fn bind_entity_create<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
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
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
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
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
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
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
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

/// The entity-subscription insert, its field values bound by
/// [`bind_entity_create`] in the marker order this emits.
fn entity_create_sql() -> Result<String, SubscriptionError> {
    let mut insert = Query::insert();
    insert.into_table(Subscriptions::Table).columns([
        Subscriptions::Id,
        Subscriptions::Kind,
        Subscriptions::Type,
        Subscriptions::SubscriptionName,
        Subscriptions::Description,
        Subscriptions::Entities,
        Subscriptions::WatchedAttributes,
        Subscriptions::NotificationTrigger,
        Subscriptions::TimeInterval,
        Subscriptions::Q,
        Subscriptions::GeoQ,
        Subscriptions::ScopeQ,
        Subscriptions::Csf,
        Subscriptions::DatasetId,
        Subscriptions::IsActive,
        Subscriptions::Notification,
        Subscriptions::ExpiresAt,
        Subscriptions::Throttling,
        Subscriptions::Status,
        Subscriptions::TemporalQ,
        Subscriptions::Lang,
        Subscriptions::JsonldContext,
        Subscriptions::LocalOnly,
        Subscriptions::CreatedAt,
        Subscriptions::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            text_literal("entity"),
            text_literal("Subscription"),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            external_placeholder(9),
            external_placeholder(10),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            external_placeholder(15),
            external_placeholder(16),
            cast(external_placeholder(17), PostgresType::SubscriptionStatus),
            external_placeholder(18),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            now(),
            now(),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The entity-subscription replacement, bound by [`bind_entity_update`].
fn entity_update_sql() -> String {
    let mut update = Query::update();
    update
        .table(Subscriptions::Table)
        .values([
            (Subscriptions::SubscriptionName, external_placeholder(2)),
            (Subscriptions::Description, external_placeholder(3)),
            (Subscriptions::Entities, external_placeholder(4)),
            (Subscriptions::WatchedAttributes, external_placeholder(5)),
            (Subscriptions::NotificationTrigger, external_placeholder(6)),
            (Subscriptions::TimeInterval, external_placeholder(7)),
            (Subscriptions::Q, external_placeholder(8)),
            (Subscriptions::GeoQ, external_placeholder(9)),
            (Subscriptions::ScopeQ, external_placeholder(10)),
            (Subscriptions::Csf, external_placeholder(11)),
            (Subscriptions::DatasetId, external_placeholder(12)),
            (Subscriptions::IsActive, external_placeholder(13)),
            (Subscriptions::Notification, external_placeholder(14)),
            (Subscriptions::ExpiresAt, external_placeholder(15)),
            (Subscriptions::Throttling, external_placeholder(16)),
            (Subscriptions::Status, cast(external_placeholder(17), PostgresType::SubscriptionStatus)),
            (Subscriptions::Lang, external_placeholder(18)),
            (Subscriptions::JsonldContext, external_placeholder(19)),
            (Subscriptions::LocalOnly, external_placeholder(20)),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(of_kind(StoredKind::Entity));
    update.to_string(PostgresQueryBuilder)
}

/// The context-source-subscription insert, bound by [`bind_csource_create`].
fn csource_create_sql() -> Result<String, SubscriptionError> {
    let mut insert = Query::insert();
    insert.into_table(Subscriptions::Table).columns([
        Subscriptions::Id,
        Subscriptions::Kind,
        Subscriptions::Type,
        Subscriptions::SubscriptionName,
        Subscriptions::Description,
        Subscriptions::Entities,
        Subscriptions::WatchedAttributes,
        Subscriptions::NotificationTrigger,
        Subscriptions::TimeInterval,
        Subscriptions::Q,
        Subscriptions::GeoQ,
        Subscriptions::ScopeQ,
        Subscriptions::Csf,
        Subscriptions::IsActive,
        Subscriptions::Notification,
        Subscriptions::ExpiresAt,
        Subscriptions::Throttling,
        Subscriptions::Status,
        Subscriptions::TemporalQ,
        Subscriptions::Lang,
        Subscriptions::JsonldContext,
        Subscriptions::CreatedAt,
        Subscriptions::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            text_literal("csource"),
            text_literal("Subscription"),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            external_placeholder(9),
            external_placeholder(10),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            external_placeholder(15),
            cast(external_placeholder(16), PostgresType::SubscriptionStatus),
            external_placeholder(17),
            external_placeholder(18),
            external_placeholder(19),
            now(),
            now(),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The context-source-subscription replacement, bound by [`bind_csource_update`].
fn csource_update_sql() -> String {
    let mut update = Query::update();
    update
        .table(Subscriptions::Table)
        .values([
            (Subscriptions::SubscriptionName, external_placeholder(2)),
            (Subscriptions::Description, external_placeholder(3)),
            (Subscriptions::Entities, external_placeholder(4)),
            (Subscriptions::WatchedAttributes, external_placeholder(5)),
            (Subscriptions::NotificationTrigger, external_placeholder(6)),
            (Subscriptions::TimeInterval, external_placeholder(7)),
            (Subscriptions::Q, external_placeholder(8)),
            (Subscriptions::GeoQ, external_placeholder(9)),
            (Subscriptions::ScopeQ, external_placeholder(10)),
            (Subscriptions::Csf, external_placeholder(11)),
            (Subscriptions::IsActive, external_placeholder(12)),
            (Subscriptions::Notification, external_placeholder(13)),
            (Subscriptions::ExpiresAt, external_placeholder(14)),
            (Subscriptions::Throttling, external_placeholder(15)),
            (Subscriptions::Status, cast(external_placeholder(16), PostgresType::SubscriptionStatus)),
            (Subscriptions::Lang, external_placeholder(17)),
            (Subscriptions::JsonldContext, external_placeholder(18)),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(of_kind(StoredKind::Csource));
    update.to_string(PostgresQueryBuilder)
}

/// Generates a `SubscriptionStore` implementation backed by the `subscriptions`
/// table, its statements built through sea-query.
macro_rules! impl_subscription_store {
    ($type:ty, kind: $kind:expr, has_dataset_id: $hdi:expr, create_sql: $create:path, update_sql: $update:path, create_binds: $create_fn:path, update_binds: $update_fn:path) => {
        #[async_trait::async_trait]
        impl SubscriptionStore for $type {
            async fn get(&self, id: &Uri) -> Result<Option<StoredSubscription>, SubscriptionError> {
                let mut select = projected_select($kind);
                select.and_where(Expr::col(Subscriptions::Id).eq(id.as_str())).and_where(of_kind($kind));
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
                    .order_by(Subscriptions::CreatedAt, Order::Asc)
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
                let statement = Query::delete()
                    .from_table(Subscriptions::Table)
                    .and_where(Expr::col(Subscriptions::Id).eq(id.as_str()))
                    .and_where(of_kind($kind))
                    .to_owned();
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
                    .and_where(Expr::col(Subscriptions::Id).eq(id.as_str()))
                    .and_where(of_kind($kind))
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
                    .and_where(Expr::col(Subscriptions::Status).eq(text_literal("active")))
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
                let mut statement = Query::update();
                statement
                    .table(Subscriptions::Table)
                    .value(Subscriptions::TimesSent, Expr::col(Subscriptions::TimesSent).add(1))
                    .value(Subscriptions::LastNotification, now())
                    .value(Subscriptions::ModifiedAt, now())
                    .and_where(Expr::col(Subscriptions::Id).eq(id.as_str()))
                    .and_where(Expr::col(Subscriptions::Status).eq(text_literal("active")))
                    .and_where(of_kind($kind));
                if let Some(ceiling) = earliest_allowed_last_notification {
                    statement.cond_where(
                        Cond::any()
                            .add(Expr::col(Subscriptions::LastNotification).is_null())
                            .add(Expr::col(Subscriptions::LastNotification).lte(ceiling)),
                    );
                }
                let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
                let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .execute(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                Ok(result.rows_affected() > 0)
            }

            async fn mark_success(&self, id: &Uri) -> Result<(), SubscriptionError> {
                let (sql, values) = mark_statement(Subscriptions::LastSuccess, id, $kind).build_sqlx(PostgresQueryBuilder);
                sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                    .execute(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                Ok(())
            }

            async fn mark_failure(&self, id: &Uri) -> Result<(), SubscriptionError> {
                let (sql, values) = mark_statement(Subscriptions::LastFailure, id, $kind).build_sqlx(PostgresQueryBuilder);
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
                    .value(Subscriptions::IsActive, Expr::val(false))
                    .value(Subscriptions::ModifiedAt, Expr::val(cutoff))
                    .and_where(Expr::col(Subscriptions::Status).ne(text_literal("expired")))
                    .and_where(Expr::col(Subscriptions::ExpiresAt).is_not_null())
                    .and_where(Expr::col(Subscriptions::ExpiresAt).lte(cutoff))
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

            async fn create_subscription(&self, id: &Uri, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), SubscriptionError> {
                let sql = $create()?;
                let query = ($create_fn)(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())), id, fields, status);
                query.execute(&self.pool).await.map_err(|error| create_error(error, id))?;
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
                    .and_where(Expr::col(Subscriptions::Status).eq(text_literal("active")))
                    .and_where(of_kind($kind))
                    .and_where(Expr::col(Subscriptions::TimeInterval).is_not_null())
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

/// The `SET last_* = NOW(), modified_at = NOW() WHERE id AND kind` touch shared
/// by `mark_success` and `mark_failure`.
fn mark_statement(column: Subscriptions, id: &Uri, kind: StoredKind) -> sea_query::UpdateStatement {
    Query::update()
        .table(Subscriptions::Table)
        .value(column, now())
        .value(Subscriptions::ModifiedAt, now())
        .and_where(Expr::col(Subscriptions::Id).eq(id.as_str()))
        .and_where(of_kind(kind))
        .to_owned()
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
        subscription_store::postgresql::{csource_create_sql, entity_create_sql, entity_update_sql},
    };
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

    #[test]
    fn the_entity_create_carries_its_kind_type_and_status_cast_with_contiguous_markers() {
        let sql = entity_create_sql().expect("the fixed entity insert builds");
        assert!(sql.contains("'entity'") && sql.contains("'Subscription'"), "{sql}");
        assert!(sql.contains("$17::subscription_status"), "{sql}");
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        for marker in 1..=21 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$22"), "the create binds exactly twenty-one values: {sql}");
    }

    #[test]
    fn the_csource_create_omits_the_dataset_and_local_only_columns() {
        let sql = csource_create_sql().expect("the fixed context-source insert builds");
        assert!(sql.contains("'csource'"), "{sql}");
        assert!(sql.contains("$16::subscription_status"), "{sql}");
        assert!(!sql.contains("$20"), "the context-source create binds nineteen values: {sql}");
        assert!(!sql.contains("dataset_id"), "a context-source subscription stores no datasetId: {sql}");
    }

    #[test]
    fn the_entity_update_pins_the_row_by_id_and_kind() {
        let sql = entity_update_sql();
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains("'entity'"), "{sql}");
        assert!(sql.contains("$17::subscription_status"), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
    }
}
