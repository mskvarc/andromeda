//! The statements over a Snapshot's control record and preparation state.
//!
//! A Snapshot is two rows -- the immutable `snapshots` control record and the
//! mutable `snapshot_preparation` lease row, sharing `snapshot_key` -- plus its
//! own branch schema named after that key. Everything here names a Snapshot by
//! its URI; the surrogate `snapshot_key` is issued by the database and used only
//! to name the branch schema and to key the preparation row.

use crate::snapshot::{
    branch_schema::{create_branch_schema, drop_branch_schema},
    mapper::row_to_snapshot,
    metadata_change::MetadataChange,
    persistence_error,
    status_outbox::enqueue_terminal_notification,
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_postgresql_context_query::snapshot_purge::build_match_query;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::{cast::cast, clock::now, pg_type::PostgresType, placeholder::external_placeholder};
use andromeda_snapshot::{error::SnapshotError, store::InsertSnapshotParams};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::{Expr, ExprTrait, Func, JoinType, Keyword, LockType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::time::Duration;
use tokio::time::sleep;
use url::Url;
use uuid::Uuid;

/// The stored-snapshot projection every read shares, over the control record
/// joined to its preparation row, decoding the status enum to text.
pub(crate) fn apply_snapshot_projection(select: &mut SelectStatement) {
    select
        .column((Snapshots::Table, Snapshots::Id))
        .column((Snapshots::Table, Snapshots::SnapshotQueries))
        .column((Snapshots::Table, Snapshots::SnapshotTemporalQueries))
        .column((Snapshots::Table, Snapshots::QueryContext))
        .column((Snapshots::Table, Snapshots::SnapshotLifetime))
        .column((Snapshots::Table, Snapshots::SnapshotPriority))
        .column((Snapshots::Table, Snapshots::Endpoint))
        .column((Snapshots::Table, Snapshots::ReceiverInfo))
        .column((Snapshots::Table, Snapshots::CloneSourceId))
        .column((Snapshots::Table, Snapshots::CreatedAt))
        .column((Snapshots::Table, Snapshots::ExpiresAt))
        .expr_as(
            cast(Expr::col((SnapshotPreparation::Table, SnapshotPreparation::Status)), PostgresType::Text),
            SnapshotPreparation::Status,
        )
        .column((SnapshotPreparation::Table, SnapshotPreparation::QueriesDetails))
        .column((SnapshotPreparation::Table, SnapshotPreparation::TemporalQueriesDetails))
        .column((SnapshotPreparation::Table, SnapshotPreparation::ModifiedAt))
        .column((SnapshotPreparation::Table, SnapshotPreparation::LastUsedAt));
}

/// A projecting select of `snapshots` joined to `snapshot_preparation`, ready
/// for a caller's `WHERE`.
pub(crate) fn snapshot_select() -> SelectStatement {
    let mut select = Query::select();
    apply_snapshot_projection(&mut select);
    select.from(Snapshots::Table).join(
        JoinType::Join,
        SnapshotPreparation::Table,
        Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
    );
    select
}

/// The immutable control columns a create binds, in placeholder order; the
/// surrogate key is issued by the database.
const fn control_columns() -> [Snapshots; 11] {
    [
        Snapshots::Id,
        Snapshots::SnapshotQueries,
        Snapshots::SnapshotTemporalQueries,
        Snapshots::QueryContext,
        Snapshots::SnapshotLifetime,
        Snapshots::SnapshotPriority,
        Snapshots::Endpoint,
        Snapshots::ReceiverInfo,
        Snapshots::CloneSourceId,
        Snapshots::CreatedAt,
        Snapshots::ExpiresAt,
    ]
}

/// The control-record insert, its eleven values bound by the caller, returning
/// the database-issued surrogate key.
fn control_insert_sql() -> Result<String, SnapshotError> {
    let mut insert = Query::insert();
    insert
        .into_table(Snapshots::Table)
        .columns(control_columns())
        .values((1..=11).map(external_placeholder))
        .map_err(persistence_error)?
        .returning_col(Snapshots::SnapshotKey);
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The preparation-record insert; the status marker is cast to the enum, the
/// lease triple starts null.
fn preparation_insert_sql() -> Result<String, SnapshotError> {
    let mut insert = Query::insert();
    insert
        .into_table(SnapshotPreparation::Table)
        .columns([
            SnapshotPreparation::SnapshotKey,
            SnapshotPreparation::Status,
            SnapshotPreparation::ModifiedAt,
            SnapshotPreparation::LastUsedAt,
            SnapshotPreparation::QueriesDetails,
            SnapshotPreparation::TemporalQueriesDetails,
        ])
        .values([
            external_placeholder(1),
            cast(external_placeholder(2), PostgresType::SnapshotStatus),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Write one Snapshot: the control record, its preparation row, and its two
/// (empty) branch schemas, all in one transaction.
pub(crate) async fn create_snapshot(pool: &PgPool, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
    let mut tx = pool.begin().await.map_err(persistence_error)?;
    let control_sql = control_insert_sql()?;
    let snapshot_key_raw: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(control_sql.as_str()))
        .bind(params.id.as_str())
        .bind(params.snapshot_queries.map(serde_json::to_value).transpose()?)
        .bind(params.snapshot_temporal_queries.map(serde_json::to_value).transpose()?)
        .bind(params.query_context_json)
        .bind(params.snapshot_lifetime.map(ToString::to_string))
        .bind(params.snapshot_priority)
        .bind(params.endpoint.map(Url::as_str))
        .bind(params.receiver_info_json)
        .bind(params.clone_source_id.map(Uri::as_str))
        .bind(params.created_at)
        .bind(params.expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| snapshot_create_error(error, params.id))?;
    let snapshot_key = SnapshotKey::try_from(snapshot_key_raw).map_err(|error| SnapshotError::internal(error.to_string()))?;
    let preparation_sql = preparation_insert_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(preparation_sql.as_str()))
        .bind(snapshot_key.as_i64())
        .bind(<&str>::from(params.status))
        .bind(params.modified_at)
        .bind(params.last_used_at)
        .bind(params.queries_details_json)
        .bind(params.temporal_queries_details_json)
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
    create_branch_schema(&mut tx, snapshot_key, SnapshotBranch::Published)
        .await
        .map_err(persistence_error)?;
    create_branch_schema(&mut tx, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(persistence_error)?;
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

/// Resolve one Snapshot's surrogate key by URI.
pub(crate) async fn snapshot_key_of<'e, E>(executor: E, id: &Uri) -> Result<Option<SnapshotKey>, SnapshotError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let statement = Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let raw = sqlx::query_scalar_with::<_, i64, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(executor)
        .await
        .map_err(persistence_error)?;
    raw.map(|raw| SnapshotKey::try_from(raw).map_err(|error| SnapshotError::internal(error.to_string())))
        .transpose()
}

/// Read one snapshot by URI.
pub(crate) async fn read_snapshot(pool: &PgPool, id: &Uri) -> Result<Option<Snapshot>, SnapshotError> {
    let mut select = snapshot_select();
    select.and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(id.as_str()));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    row.as_ref().map(row_to_snapshot).transpose().map_err(persistence_error)
}

/// Whether a snapshot with this URI exists.
pub(crate) async fn snapshot_exists(pool: &PgPool, id: &Uri) -> Result<bool, SnapshotError> {
    Ok(snapshot_key_of(pool, id).await?.is_some())
}

/// Resolve the published branch owned by one Snapshot URI.
pub(crate) async fn resolve_snapshot_target(pool: &PgPool, id: &Uri) -> Result<Option<SnapshotTarget>, SnapshotError> {
    Ok(snapshot_key_of(pool, id).await?.map(|key| SnapshotTarget::published(key, id.clone())))
}

/// Resolve the private staging branch populated while a capture is preparing.
pub(crate) async fn resolve_snapshot_staging_target(pool: &PgPool, id: &Uri) -> Result<Option<SnapshotTarget>, SnapshotError> {
    Ok(snapshot_key_of(pool, id).await?.map(|key| SnapshotTarget::staging(key, id.clone())))
}

/// `status = 'preparing'::snapshot_status` on the preparation row: the column
/// is the PostgreSQL enum, so the literal is cast to it.
pub(crate) fn status_is_preparing() -> SimpleExpr {
    Expr::col((SnapshotPreparation::Table, SnapshotPreparation::Status))
        .eq(cast(Expr::val(<&str>::from(SnapshotStatus::Preparing)), PostgresType::SnapshotStatus))
}

/// Apply the mutable metadata of one snapshot, keeping whatever the caller
/// left unspecified; the modification instant lives on the preparation row.
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
        );
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
        bump_modified_at(&mut transaction, id).await?;
        enqueue_terminal_notification(&mut transaction, id).await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Stamp the preparation row's modification instant for one Snapshot URI.
async fn bump_modified_at(transaction: &mut Transaction<'_, Postgres>, id: &Uri) -> Result<(), SnapshotError> {
    let statement = Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::ModifiedAt, now())
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).in_subquery(snapshot_key_subquery(id)))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// `SELECT snapshot_key FROM snapshots WHERE id = ?`, for a preparation-row
/// mutation that reaches its row through the control record's URI.
fn snapshot_key_subquery(id: &Uri) -> SelectStatement {
    Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned()
}

/// Move one preparing snapshot to its final status, recording the per-query
/// details; a failure discards the private staging branch.
pub(crate) async fn update_status(
    pool: &PgPool,
    id: &Uri,
    claimed_by: Uuid,
    status: SnapshotStatus,
    queries_details: Option<&[ExecutionResultDetails]>,
    temporal_queries_details: Option<&[ExecutionResultDetails]>,
) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let Some(snapshot_key) = snapshot_key_of(&mut *transaction, id).await? else {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(0);
    };
    let statement = Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::Status, cast(Expr::val(<&str>::from(status)), PostgresType::SnapshotStatus))
        .value(
            SnapshotPreparation::QueriesDetails,
            Expr::val(queries_details.and_then(|d| serde_json::to_value(d).ok())),
        )
        .value(
            SnapshotPreparation::TemporalQueriesDetails,
            Expr::val(temporal_queries_details.and_then(|d| serde_json::to_value(d).ok())),
        )
        .value(SnapshotPreparation::ModifiedAt, now())
        .value(SnapshotPreparation::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(SnapshotPreparation::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(SnapshotPreparation::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(snapshot_key.as_i64()))
        .and_where(Expr::col(SnapshotPreparation::ClaimedBy).eq(claimed_by))
        .and_where(status_is_preparing())
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if result.rows_affected() == 1 {
        enqueue_terminal_notification(&mut transaction, id).await?;
        if status == SnapshotStatus::Failure {
            drop_branch_schema(&mut transaction, snapshot_key, SnapshotBranch::Staging)
                .await
                .map_err(persistence_error)?;
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

/// Bounded retries for a branch-reaping transaction that a deadlock rolls back.
///
/// Dropping a Snapshot's branch schema takes locks that can deadlock against the
/// notification delivery worker draining that branch's pending deliveries;
/// PostgreSQL rolls the victim's whole transaction back, so re-running it from a
/// fresh transaction clears the contention. Kept small because heavy contention
/// is rare and each attempt re-runs the entire delete.
const BRANCH_REAP_ATTEMPTS: u32 = 5;
/// Backoff between contended branch-reaping attempts.
const BRANCH_REAP_BACKOFF: Duration = Duration::from_millis(50);

/// Delete one snapshot by identifier: drop its branch schemas, then delete the
/// control record, which cascades the preparation and outbox rows. Retries a
/// transaction a deadlock with branch delivery rolls back.
pub(crate) async fn delete_snapshot(pool: &PgPool, id: &Uri) -> Result<u64, SnapshotError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match delete_snapshot_once(pool, id).await {
            Err(SnapshotError::Contended) if attempt < BRANCH_REAP_ATTEMPTS => sleep(BRANCH_REAP_BACKOFF).await,
            outcome => return outcome,
        }
    }
}

/// Delete one Snapshot's control record and branch schema in a single
/// transaction, returning the number of control rows removed.
async fn delete_snapshot_once(pool: &PgPool, id: &Uri) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let statement = Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .lock(LockType::Update)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let Some(raw) = sqlx::query_scalar_with::<_, i64, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?
    else {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(0);
    };
    let snapshot_key = SnapshotKey::try_from(raw).map_err(|error| SnapshotError::internal(error.to_string()))?;
    reap_branch(&mut transaction, snapshot_key).await?;
    let deleted = delete_control_record(&mut transaction, snapshot_key).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(deleted)
}

/// Delete every snapshot whose expiry has passed `cutoff`.
pub(crate) async fn delete_expired_snapshots(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match delete_expired_snapshots_once(pool, cutoff).await {
            Err(SnapshotError::Contended) if attempt < BRANCH_REAP_ATTEMPTS => sleep(BRANCH_REAP_BACKOFF).await,
            outcome => return outcome,
        }
    }
}

/// Delete every Snapshot past `cutoff` and its branch schema in a single
/// transaction, returning the number of control rows removed.
async fn delete_expired_snapshots_once(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let statement = Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::ExpiresAt).lte(cutoff))
        .lock(LockType::Update)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_scalar_with::<_, i64, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if rows.is_empty() {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(0);
    }
    let mut deleted = 0;
    for raw in rows {
        let snapshot_key = SnapshotKey::try_from(raw).map_err(|error| SnapshotError::internal(error.to_string()))?;
        reap_branch(&mut transaction, snapshot_key).await?;
        deleted += delete_control_record(&mut transaction, snapshot_key).await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(deleted)
}

/// Drop both possible branch schemas of one Snapshot; `IF EXISTS` covers the
/// staging schema a promoted Snapshot no longer has.
pub(crate) async fn reap_branch(transaction: &mut Transaction<'_, Postgres>, snapshot_key: SnapshotKey) -> Result<(), SnapshotError> {
    drop_branch_schema(transaction, snapshot_key, SnapshotBranch::Published)
        .await
        .map_err(persistence_error)?;
    drop_branch_schema(transaction, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// Delete one control record by key, cascading its preparation and outbox rows.
pub(crate) async fn delete_control_record(transaction: &mut Transaction<'_, Postgres>, snapshot_key: SnapshotKey) -> Result<u64, SnapshotError> {
    let statement = Query::delete()
        .from_table(Snapshots::Table)
        .and_where(Expr::col(Snapshots::SnapshotKey).eq(snapshot_key.as_i64()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Stamp the last-used instant of one snapshot on its preparation row.
pub(crate) async fn touch_last_used(pool: &PgPool, id: &Uri) -> Result<(), SnapshotError> {
    let statement = Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::LastUsedAt, now())
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).in_subquery(snapshot_key_subquery(id)))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// The deterministic victim order shared by quota enforcement: lower priority,
/// earlier expiry, older last use, then lexicographically smaller URI.
pub(crate) fn victim_order(select: &mut SelectStatement) {
    select
        .order_by((Snapshots::Table, Snapshots::SnapshotPriority), Order::Asc)
        .order_by((Snapshots::Table, Snapshots::ExpiresAt), Order::Asc)
        .order_by_with_nulls(
            (SnapshotPreparation::Table, SnapshotPreparation::LastUsedAt),
            Order::Asc,
            sea_query::NullOrdering::First,
        )
        .order_by((Snapshots::Table, Snapshots::Id), Order::Asc);
}

#[cfg(test)]
mod tests {
    use crate::snapshot::lifecycle::{control_insert_sql, preparation_insert_sql, snapshot_select};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_control_insert_binds_eleven_values_and_returns_the_surrogate_key() {
        let sql = control_insert_sql().expect("the control insert builds");
        for marker in 1..=11 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$12"), "the control insert binds exactly eleven values: {sql}");
        assert!(sql.contains("RETURNING \"snapshot_key\""), "the surrogate key is returned: {sql}");
        assert!(!sql.contains("context_space_key"), "no context-space key remains: {sql}");
        assert!(!sql.contains("\"status\""), "the status lives on the preparation row: {sql}");
    }

    #[test]
    fn the_preparation_insert_casts_its_status_marker() {
        let sql = preparation_insert_sql().expect("the preparation insert builds");
        assert!(sql.contains("$2::snapshot_status"), "{sql}");
        assert!(sql.contains("snapshot_preparation"), "{sql}");
    }

    #[test]
    fn the_read_projection_joins_the_preparation_row() {
        let sql = snapshot_select().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("FROM \"snapshots\" JOIN \"snapshot_preparation\" ON \"snapshot_preparation\".\"snapshot_key\" = \"snapshots\".\"snapshot_key\""),
            "{sql}"
        );
        assert!(sql.contains("\"snapshot_preparation\".\"status\"::text"), "{sql}");
    }
}
