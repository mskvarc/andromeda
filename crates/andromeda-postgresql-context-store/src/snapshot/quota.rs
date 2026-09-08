//! Deterministic tenant-local Snapshot quota enforcement.

use crate::{context_space::delete_context_space_statement, snapshot::persistence_error};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::{
    current::{CurrentAttributeValues, Entities, EntityDocuments},
    notification::{NotificationDeliveries, NotificationEvents},
    snapshot::Snapshots,
    subscriptions::Subscriptions,
    temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{advisory_lock::advisory_xact_lock, introspection::jsonb_row_size};
use andromeda_snapshot::error::SnapshotError;
use sea_query::{Expr, ExprTrait, Func, LockBehavior, LockType, NullOrdering, Order, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Row, Transaction};

/// Remove deterministic victims until both configured limits are satisfied.
pub(crate) async fn enforce_snapshot_quotas(pool: &PgPool, count_quota: Option<usize>, byte_quota: Option<u64>) -> Result<Vec<Uri>, SnapshotError> {
    if count_quota.is_none() && byte_quota.is_none() {
        return Ok(Vec::new());
    }
    let count_quota = count_quota
        .map(i64::try_from)
        .transpose()
        .map_err(|error| SnapshotError::internal(format!("Snapshot count quota is out of range: {error}")))?;
    let byte_quota = byte_quota
        .map(i64::try_from)
        .transpose()
        .map_err(|error| SnapshotError::internal(format!("Snapshot byte quota is out of range: {error}")))?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let lock = Query::select().expr(advisory_xact_lock(Expr::val("andromeda.snapshot.quota"))).to_owned();
    execute(&mut transaction, lock).await?;
    let mut evicted = Vec::new();
    loop {
        let count_statement = Query::select()
            .expr(Func::count(Expr::col(Snapshots::SnapshotKey)))
            .from(Snapshots::Table)
            .to_owned();
        let (sql, values) = count_statement.build_sqlx(PostgresQueryBuilder);
        let count: i64 = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        let bytes = snapshot_bytes(&mut transaction).await?;
        if count_quota.is_none_or(|maximum| count <= maximum) && byte_quota.is_none_or(|maximum| bytes <= maximum) {
            break;
        }
        let victim = Query::select()
            .columns([Snapshots::Id, Snapshots::ContextSpaceKey, Snapshots::StagingContextSpaceKey])
            .from(Snapshots::Table)
            .order_by(Snapshots::SnapshotPriority, Order::Asc)
            .order_by(Snapshots::ExpiresAt, Order::Asc)
            .order_by_with_nulls(Snapshots::LastUsedAt, Order::Asc, NullOrdering::First)
            .order_by(Snapshots::Id, Order::Asc)
            .limit(1)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .to_owned();
        let (sql, values) = victim.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| SnapshotError::internal("Snapshot quota was exceeded without an evictable Snapshot"))?;
        let raw_id: String = row.try_get("id").map_err(persistence_error)?;
        let context_space_key: i64 = row.try_get("context_space_key").map_err(persistence_error)?;
        let staging_context_space_key: Option<i64> = row.try_get("staging_context_space_key").map_err(persistence_error)?;
        if let Some(staging_key) = staging_context_space_key {
            execute(&mut transaction, delete_context_space_statement(staging_key)).await?;
        }
        execute(&mut transaction, delete_context_space_statement(context_space_key)).await?;
        evicted.push(
            raw_id
                .parse()
                .map_err(|error| SnapshotError::internal(format!("Invalid Snapshot URI selected for eviction: {error}")))?,
        );
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(evicted)
}

/// Execute one `SeaQuery` statement inside the quota transaction.
async fn execute(transaction: &mut Transaction<'_, Postgres>, statement: impl SqlxBinder) -> Result<u64, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Sum deterministic logical row sizes for every Snapshot-owned branch relation.
async fn snapshot_bytes(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, SnapshotError> {
    let keys = Query::select()
        .columns([Snapshots::ContextSpaceKey, Snapshots::StagingContextSpaceKey])
        .from(Snapshots::Table)
        .to_owned();
    let (sql, values) = keys.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    let mut total = relation_bytes(transaction, Snapshots::Table, Snapshots::SnapshotKey, None).await?;
    for row in rows {
        let published: i64 = row.try_get("context_space_key").map_err(persistence_error)?;
        let staging: Option<i64> = row.try_get("staging_context_space_key").map_err(persistence_error)?;
        for key in [Some(published), staging].into_iter().flatten() {
            total += relation_bytes(transaction, Entities::Table, Entities::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(transaction, EntityDocuments::Table, EntityDocuments::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(transaction, CurrentAttributeValues::Table, CurrentAttributeValues::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(transaction, TemporalEntities::Table, TemporalEntities::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(
                transaction,
                TemporalAttributeStreams::Table,
                TemporalAttributeStreams::ContextSpaceKey,
                Some(key),
            )
            .await?;
            total += relation_bytes(transaction, AttributeHistory::Table, AttributeHistory::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(transaction, ScopeHistory::Table, ScopeHistory::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(transaction, Subscriptions::Table, Subscriptions::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(transaction, NotificationEvents::Table, NotificationEvents::ContextSpaceKey, Some(key)).await?;
            total += relation_bytes(transaction, NotificationDeliveries::Table, NotificationDeliveries::ContextSpaceKey, Some(key)).await?;
        }
    }
    Ok(total)
}

/// Sum logical row sizes for one relation, optionally restricted to one branch key.
async fn relation_bytes<T, C>(transaction: &mut Transaction<'_, Postgres>, table: T, context_column: C, key: Option<i64>) -> Result<i64, SnapshotError>
where
    T: sea_query::Iden + Copy + 'static,
    C: sea_query::Iden + Copy + 'static,
{
    let mut statement = Query::select();
    statement
        .expr(Func::coalesce([SimpleExpr::from(Func::sum(jsonb_row_size(Expr::col(table)))), Expr::val(0)]))
        .from(table);
    if let Some(key) = key {
        statement.and_where(Expr::col(context_column).eq(key));
    }
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut **transaction)
        .await
        .map_err(persistence_error)
}
