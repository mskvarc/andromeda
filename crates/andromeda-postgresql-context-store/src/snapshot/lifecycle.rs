//! The statements over the `snapshots` control record: create, read, clone,
//! update and delete of one snapshot's own row.
//!
//! The child rows a snapshot captured are the concern of
//! [`crate::snapshot::content`]; the summaries read over them of
//! [`crate::snapshot::introspection`]. Everything here names a snapshot by its
//! URI: the surrogate `snapshot_key` is issued by the database and never
//! crosses this boundary.

use crate::{
    context_space::delete_context_space_statement,
    snapshot::{mapper::row_to_snapshot, metadata_change::MetadataChange, persistence_error, status_outbox::enqueue_terminal_notification},
};
use andromeda_context_space::{
    context_space::{ContextSpace, ContextSpaceKind},
    context_space_key::ContextSpaceKey,
    snapshot_target::SnapshotTarget,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_postgresql_context_query::snapshot_purge::build_match_query;
use andromeda_postgresql_schema::{context_space::ContextSpaces, snapshot::Snapshots};
use andromeda_postgresql_seaquery::{cast::cast, clock::now, pg_type::PostgresType, placeholder::external_placeholder};
use andromeda_snapshot::{error::SnapshotError, store::InsertSnapshotParams};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::{Expr, ExprTrait, Func, Iden, Keyword, LockType, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Row, Transaction};
use url::Url;
use uuid::Uuid;

/// The stored-snapshot projection every read shares, decoding the status enum to
/// text under the column name the mapper reads it by.
pub(crate) fn apply_snapshot_projection(select: &mut SelectStatement) {
    select
        .column(Snapshots::Id)
        .column(Snapshots::StagingContextSpaceKey)
        .column(Snapshots::SnapshotQueries)
        .column(Snapshots::SnapshotTemporalQueries)
        .column(Snapshots::QueryContext)
        .column(Snapshots::SnapshotLifetime)
        .column(Snapshots::SnapshotPriority)
        .column(Snapshots::Endpoint)
        .column(Snapshots::ReceiverInfo)
        .expr_as(cast(Expr::col(Snapshots::Status), PostgresType::Text), Snapshots::Status)
        .column(Snapshots::CloneSourceId)
        .column(Snapshots::QueriesDetails)
        .column(Snapshots::TemporalQueriesDetails)
        .column(Snapshots::CreatedAt)
        .column(Snapshots::ModifiedAt)
        .column(Snapshots::ExpiresAt)
        .column(Snapshots::LastUsedAt);
}

/// A projecting select of `snapshots`, ready for a caller's `WHERE`.
pub(crate) fn snapshot_select() -> SelectStatement {
    let mut select = Query::select();
    apply_snapshot_projection(&mut select);
    select.from(Snapshots::Table);
    select
}

/// The columns a control record is written with, in the order the create and
/// the clone bind them; the surrogate key is issued by the database.
const fn control_columns() -> [Snapshots; 18] {
    [
        Snapshots::ContextSpaceKey,
        Snapshots::StagingContextSpaceKey,
        Snapshots::Id,
        Snapshots::SnapshotQueries,
        Snapshots::SnapshotTemporalQueries,
        Snapshots::QueryContext,
        Snapshots::SnapshotLifetime,
        Snapshots::SnapshotPriority,
        Snapshots::Endpoint,
        Snapshots::ReceiverInfo,
        Snapshots::Status,
        Snapshots::CloneSourceId,
        Snapshots::QueriesDetails,
        Snapshots::TemporalQueriesDetails,
        Snapshots::CreatedAt,
        Snapshots::ModifiedAt,
        Snapshots::ExpiresAt,
        Snapshots::LastUsedAt,
    ]
}

/// The snapshot insert, its eighteen values bound by the caller's chain; the
/// status marker is cast to the `snapshot_status` enum.
pub(crate) fn snapshot_create_sql() -> Result<String, SnapshotError> {
    let mut insert = Query::insert();
    insert.into_table(Snapshots::Table).columns(control_columns());
    insert
        .values([
            external_placeholder(1),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            external_placeholder(9),
            external_placeholder(10),
            cast(external_placeholder(11), PostgresType::SnapshotStatus),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            external_placeholder(15),
            external_placeholder(16),
            external_placeholder(17),
            external_placeholder(18),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Write one control record.
pub(crate) async fn create_snapshot(pool: &PgPool, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
    let mut tx = pool.begin().await.map_err(persistence_error)?;
    let context_space_key = create_snapshot_context_space(&mut tx).await?;
    let staging_context_space_key = Some(create_snapshot_context_space(&mut tx).await?);
    let sql = snapshot_create_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(context_space_key.as_i64())
        .bind(staging_context_space_key.map(ContextSpaceKey::as_i64))
        .bind(params.id.as_str())
        .bind(params.snapshot_queries.map(serde_json::to_value).transpose()?)
        .bind(params.snapshot_temporal_queries.map(serde_json::to_value).transpose()?)
        .bind(params.query_context_json)
        .bind(params.snapshot_lifetime.map(ToString::to_string))
        .bind(params.snapshot_priority)
        .bind(params.endpoint.map(Url::as_str))
        .bind(params.receiver_info_json)
        .bind(<&str>::from(params.status))
        .bind(params.clone_source_id.map(Uri::as_str))
        .bind(params.queries_details_json)
        .bind(params.temporal_queries_details_json)
        .bind(params.created_at)
        .bind(params.modified_at)
        .bind(params.expires_at)
        .bind(params.last_used_at)
        .execute(&mut *tx)
        .await
        .map_err(|error| snapshot_create_error(error, params.id))?;
    tx.commit().await.map_err(persistence_error)?;
    Ok(())
}

/// Preserve the protocol-visible duplicate-id classification from PostgreSQL.
fn snapshot_create_error(error: sqlx::Error, id: &Uri) -> SnapshotError {
    if let sqlx::Error::Database(database_error) = &error
        && database_error.code().as_deref() == Some("23505")
        && database_error.constraint() == Some("snapshots_id_key")
    {
        return SnapshotError::already_exists(id);
    }
    persistence_error(error)
}

/// Allocate the parent branch for one Snapshot in the caller's transaction.
pub(crate) async fn create_snapshot_context_space(tx: &mut Transaction<'_, Postgres>) -> Result<ContextSpaceKey, SnapshotError> {
    let mut insert = Query::insert();
    insert
        .into_table(ContextSpaces::Table)
        .columns([ContextSpaces::Kind])
        .values([cast(Expr::val("snapshot"), PostgresType::ContextSpaceKind)])
        .map_err(persistence_error)?
        .returning_col(ContextSpaces::ContextSpaceKey);
    let (sql, values) = insert.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut **tx)
        .await
        .map_err(persistence_error)?;
    let raw = row.try_get::<i64, _>("context_space_key").map_err(persistence_error)?;
    ContextSpaceKey::try_from(raw).map_err(|error| SnapshotError::internal(error.to_string()))
}

/// Read one snapshot by URI.
pub(crate) async fn read_snapshot(pool: &PgPool, id: &Uri) -> Result<Option<Snapshot>, SnapshotError> {
    let mut select = snapshot_select();
    select.and_where(Expr::col(Snapshots::Id).eq(id.as_str()));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    row.as_ref().map(row_to_snapshot).transpose().map_err(persistence_error)
}

/// Whether a snapshot with this URI exists.
pub(crate) async fn snapshot_exists(pool: &PgPool, id: &Uri) -> Result<bool, SnapshotError> {
    let statement = Query::select()
        .expr(Expr::val(1))
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    Ok(row.is_some())
}

/// Resolve the persisted branch owned by one Snapshot URI.
pub(crate) async fn resolve_snapshot_target(pool: &PgPool, id: &Uri) -> Result<Option<SnapshotTarget>, SnapshotError> {
    resolve_snapshot_context_target(pool, id, Snapshots::ContextSpaceKey).await
}

/// Resolve the private branch populated while a capture remains preparing.
pub(crate) async fn resolve_snapshot_staging_target(pool: &PgPool, id: &Uri) -> Result<Option<SnapshotTarget>, SnapshotError> {
    resolve_snapshot_context_target(pool, id, Snapshots::StagingContextSpaceKey).await
}

/// Resolve one of the control row's context-space columns as a typed Snapshot target.
async fn resolve_snapshot_context_target(pool: &PgPool, id: &Uri, column: Snapshots) -> Result<Option<SnapshotTarget>, SnapshotError> {
    let statement = Query::select()
        .column(column)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let column_name = column.to_string();
    let Some(raw) = row.try_get::<Option<i64>, _>(column_name.as_str()).map_err(persistence_error)? else {
        return Ok(None);
    };
    let key = ContextSpaceKey::try_from(raw).map_err(|error| SnapshotError::internal(error.to_string()))?;
    SnapshotTarget::new(ContextSpace::new(key, ContextSpaceKind::Snapshot), id.clone())
        .map(Some)
        .map_err(|error| SnapshotError::internal(error.to_string()))
}

/// `status = 'preparing'::snapshot_status`: the column is the PostgreSQL enum,
/// so the literal is cast to it rather than bound as text, which the enum has
/// no equality operator for.
pub(crate) fn status_is_preparing() -> SimpleExpr {
    Expr::col(Snapshots::Status).eq(cast(Expr::val(<&str>::from(SnapshotStatus::Preparing)), PostgresType::SnapshotStatus))
}

/// Execute one generated lifecycle mutation inside an existing transaction.
async fn execute_statement(transaction: &mut Transaction<'_, Postgres>, statement: impl SqlxBinder) -> Result<u64, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Apply the mutable metadata of one snapshot, keeping whatever the caller
/// left unspecified and preserving any active preparation lease.
pub(crate) async fn update_snapshot_metadata(
    pool: &PgPool,
    id: &Uri,
    lifetime: Option<&Iso8601Duration>,
    priority: Option<SnapshotPriority>,
    endpoint: MetadataChange<&Url>,
    receiver_info: MetadataChange<serde_json::Value>,
    new_expires_at: Option<DateTime<Utc>>,
) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let mut statement = Query::update();
    statement
        .table(Snapshots::Table)
        .value(
            Snapshots::SnapshotLifetime,
            Func::coalesce([Expr::val(lifetime.map(ToString::to_string)), Expr::col(Snapshots::SnapshotLifetime)]),
        )
        .value(
            Snapshots::SnapshotPriority,
            Func::coalesce([Expr::val(priority.map(|p| i16::from(p.as_u8()))), Expr::col(Snapshots::SnapshotPriority)]),
        )
        .value(
            Snapshots::ExpiresAt,
            Func::coalesce([Expr::val(new_expires_at), Expr::col(Snapshots::ExpiresAt)]),
        )
        .value(Snapshots::ModifiedAt, now());
    match endpoint {
        MetadataChange::Unchanged => {}
        MetadataChange::Cleared => {
            statement.value(Snapshots::Endpoint, None::<String>);
        }
        MetadataChange::Set(endpoint) => {
            statement.value(Snapshots::Endpoint, Some(endpoint.to_string()));
        }
    }
    match receiver_info {
        MetadataChange::Unchanged => {}
        MetadataChange::Cleared => {
            statement.value(Snapshots::ReceiverInfo, None::<serde_json::Value>);
        }
        MetadataChange::Set(receiver_info) => {
            statement.value(Snapshots::ReceiverInfo, Some(receiver_info));
        }
    }
    statement.and_where(Expr::col(Snapshots::Id).eq(id.as_str()));
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if result.rows_affected() == 1 {
        enqueue_terminal_notification(&mut transaction, id).await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Move one preparing snapshot to its final status, recording the per-query
/// details of the run.
pub(crate) async fn update_status(
    pool: &PgPool,
    id: &Uri,
    claimed_by: Uuid,
    status: SnapshotStatus,
    queries_details: Option<&[ExecutionResultDetails]>,
    temporal_queries_details: Option<&[ExecutionResultDetails]>,
) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let discarded_staging_key = if status == SnapshotStatus::Failure {
        let statement = Query::select()
            .column(Snapshots::StagingContextSpaceKey)
            .from(Snapshots::Table)
            .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
            .and_where(Expr::col(Snapshots::ClaimedBy).eq(claimed_by))
            .and_where(status_is_preparing())
            .lock(LockType::Update)
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_scalar_with::<_, Option<i64>, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence_error)?
            .flatten()
    } else {
        None
    };
    let statement = Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::Status, cast(Expr::val(<&str>::from(status)), PostgresType::SnapshotStatus))
        .value(Snapshots::QueriesDetails, Expr::val(queries_details.and_then(|d| serde_json::to_value(d).ok())))
        .value(
            Snapshots::TemporalQueriesDetails,
            Expr::val(temporal_queries_details.and_then(|d| serde_json::to_value(d).ok())),
        )
        .value(Snapshots::ModifiedAt, now())
        .value(Snapshots::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(Snapshots::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(Snapshots::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(Snapshots::StagingContextSpaceKey, SimpleExpr::Keyword(Keyword::Null))
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .and_where(Expr::col(Snapshots::ClaimedBy).eq(claimed_by))
        .and_where(status_is_preparing())
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if result.rows_affected() == 1 {
        enqueue_terminal_notification(&mut transaction, id).await?;
        if let Some(staging_key) = discarded_staging_key {
            execute_statement(&mut transaction, delete_context_space_statement(staging_key)).await?;
        }
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// The identifiers of every snapshot a purge filter (clause 5.16.7) matches.
pub(crate) async fn match_snapshots_by_filter(pool: &PgPool, filter: &SnapshotPurgeFilter) -> Result<Vec<Uri>, SnapshotError> {
    let (sql, values) = build_match_query(filter).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        let raw: String = row.try_get("id").map_err(persistence_error)?;
        let parsed = raw
            .parse::<Uri>()
            .map_err(|e| SnapshotError::internal(format!("Invalid snapshot id '{raw}' in snapshots table: {e}")))?;
        ids.push(parsed);
    }
    Ok(ids)
}

/// Delete one snapshot by identifier; its captured rows cascade with it.
pub(crate) async fn delete_snapshot(pool: &PgPool, id: &Uri) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let statement = Query::select()
        .columns([Snapshots::ContextSpaceKey, Snapshots::StagingContextSpaceKey])
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .lock(LockType::Update)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let owned = sqlx::query_as_with::<_, (i64, Option<i64>), _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let Some((context_space_key, staging_context_space_key)) = owned else {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(0);
    };
    if let Some(staging_key) = staging_context_space_key {
        execute_statement(&mut transaction, delete_context_space_statement(staging_key)).await?;
    }
    let deleted = execute_statement(&mut transaction, delete_context_space_statement(context_space_key)).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(deleted)
}

/// Delete every snapshot whose expiry has passed `cutoff`.
pub(crate) async fn delete_expired_snapshots(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let statement = Query::select()
        .columns([Snapshots::ContextSpaceKey, Snapshots::StagingContextSpaceKey])
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::ExpiresAt).lte(cutoff))
        .lock(LockType::Update)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_as_with::<_, (i64, Option<i64>), _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if rows.is_empty() {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(0);
    }
    for staging_key in rows.iter().filter_map(|(_, staging)| *staging) {
        execute_statement(&mut transaction, delete_context_space_statement(staging_key)).await?;
    }
    let mut deleted = 0;
    for context_key in rows.iter().map(|(context, _)| *context) {
        deleted += execute_statement(&mut transaction, delete_context_space_statement(context_key)).await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(deleted)
}

/// Stamp the last-used instant of one snapshot.
pub(crate) async fn touch_last_used(pool: &PgPool, id: &Uri) -> Result<(), SnapshotError> {
    let statement = Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::LastUsedAt, now())
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::snapshot::lifecycle::snapshot_create_sql;

    #[test]
    fn the_create_casts_its_status_marker_and_stamps_no_clock() {
        let sql = snapshot_create_sql().expect("the fixed snapshot insert builds");
        assert!(sql.contains("$11::snapshot_status"), "{sql}");
        for marker in 1..=18 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$19"), "the create binds exactly eighteen values: {sql}");
        assert!(sql.contains("context_space_key"), "the branch parent is explicit: {sql}");
        assert!(sql.contains("staging_context_space_key"), "the private capture branch is explicit: {sql}");
        assert!(sql.contains("clone_source_id"), "the durable preparation kind is explicit: {sql}");
        assert!(
            !sql.contains("snapshot_key"),
            "the snapshot surrogate is issued by the database, never written: {sql}"
        );
    }
}
