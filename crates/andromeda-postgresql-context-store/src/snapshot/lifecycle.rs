//! The statements over the `snapshots` control record: create, read, clone,
//! update and delete of one snapshot's own row.
//!
//! The child rows a snapshot captured are the concern of
//! [`crate::snapshot::content`]; the summaries read over them of
//! [`crate::snapshot::introspection`]. Everything here names a snapshot by its
//! URI: the surrogate `snapshot_key` is issued by the database and never
//! crosses this boundary.

use crate::snapshot::{mapper::row_to_snapshot, persistence_error};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_postgresql_context_query::snapshot_purge::build_match_query;
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::{cast::cast, clock::now, pg_type::PostgresType, placeholder::external_placeholder};
use andromeda_snapshot::{error::SnapshotError, store::InsertSnapshotParams};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::{Expr, ExprTrait, Func, Keyword, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};
use url::Url;

/// The stored-snapshot projection every read shares, decoding the status enum to
/// text under the column name the mapper reads it by.
fn apply_snapshot_projection(select: &mut SelectStatement) {
    select
        .column(Snapshots::Id)
        .column(Snapshots::SnapshotQueries)
        .column(Snapshots::SnapshotTemporalQueries)
        .column(Snapshots::SnapshotLifetime)
        .column(Snapshots::SnapshotPriority)
        .column(Snapshots::Endpoint)
        .column(Snapshots::ReceiverInfo)
        .expr_as(cast(Expr::col(Snapshots::Status), PostgresType::Text), Snapshots::Status)
        .column(Snapshots::QueriesDetails)
        .column(Snapshots::TemporalQueriesDetails)
        .column(Snapshots::CreatedAt)
        .column(Snapshots::ModifiedAt)
        .column(Snapshots::ExpiresAt)
        .column(Snapshots::LastUsedAt);
}

/// A projecting select of `snapshots`, ready for a caller's `WHERE`.
fn snapshot_select() -> SelectStatement {
    let mut select = Query::select();
    apply_snapshot_projection(&mut select);
    select.from(Snapshots::Table);
    select
}

/// The columns a control record is written with, in the order the create and
/// the clone bind them; the surrogate key is issued by the database.
const fn control_columns() -> [Snapshots; 14] {
    [
        Snapshots::Id,
        Snapshots::SnapshotQueries,
        Snapshots::SnapshotTemporalQueries,
        Snapshots::SnapshotLifetime,
        Snapshots::SnapshotPriority,
        Snapshots::Endpoint,
        Snapshots::ReceiverInfo,
        Snapshots::Status,
        Snapshots::QueriesDetails,
        Snapshots::TemporalQueriesDetails,
        Snapshots::CreatedAt,
        Snapshots::ModifiedAt,
        Snapshots::ExpiresAt,
        Snapshots::LastUsedAt,
    ]
}

/// The snapshot insert, its fourteen values bound by the caller's chain; the
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
            cast(external_placeholder(8), PostgresType::SnapshotStatus),
            external_placeholder(9),
            external_placeholder(10),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Copy the control record of `$1` into a new snapshot `$2` expiring at `$3`.
pub(crate) fn clone_snapshot_sql() -> Result<String, SnapshotError> {
    let mut select = Query::select();
    select
        .expr(external_placeholder(2))
        .column(Snapshots::SnapshotQueries)
        .column(Snapshots::SnapshotTemporalQueries)
        .column(Snapshots::SnapshotLifetime)
        .column(Snapshots::SnapshotPriority)
        .column(Snapshots::Endpoint)
        .column(Snapshots::ReceiverInfo)
        .column(Snapshots::Status)
        .column(Snapshots::QueriesDetails)
        .column(Snapshots::TemporalQueriesDetails)
        .expr(now())
        .expr(now())
        .expr(external_placeholder(3))
        .expr(SimpleExpr::Keyword(Keyword::Null))
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(external_placeholder(1)));
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(Snapshots::Table).columns(control_columns());
    insert.select_from(select).map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Write one control record.
pub(crate) async fn create_snapshot(pool: &PgPool, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
    let sql = snapshot_create_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(params.id.as_str())
        .bind(params.snapshot_queries.map(serde_json::to_value).transpose()?)
        .bind(params.snapshot_temporal_queries.map(serde_json::to_value).transpose()?)
        .bind(params.snapshot_lifetime.map(ToString::to_string))
        .bind(params.snapshot_priority)
        .bind(params.endpoint.map(Url::as_str))
        .bind(params.receiver_info_json)
        .bind(<&str>::from(params.status))
        .bind(params.queries_details_json)
        .bind(params.temporal_queries_details_json)
        .bind(params.created_at)
        .bind(params.modified_at)
        .bind(params.expires_at)
        .bind(params.last_used_at)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(())
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

/// `status = 'preparing'::snapshot_status`: the column is the PostgreSQL enum,
/// so the literal is cast to it rather than bound as text, which the enum has
/// no equality operator for.
fn status_is_preparing() -> SimpleExpr {
    Expr::col(Snapshots::Status).eq(cast(Expr::val(<&str>::from(SnapshotStatus::Preparing)), PostgresType::SnapshotStatus))
}

/// The statement selecting the next snapshot awaiting preparation.
fn next_preparing_sql() -> SelectStatement {
    let mut select = snapshot_select();
    select
        .and_where(status_is_preparing())
        .order_by(Snapshots::SnapshotPriority, Order::Desc)
        .order_by(Snapshots::CreatedAt, Order::Asc)
        .limit(1);
    select
}

/// The next snapshot awaiting preparation: highest priority first, oldest
/// within a priority.
pub(crate) async fn next_preparing_snapshot(pool: &PgPool) -> Result<Option<Snapshot>, SnapshotError> {
    let select = next_preparing_sql();
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    row.as_ref().map(row_to_snapshot).transpose().map_err(persistence_error)
}

/// Update the lifetime, priority and expiry of one snapshot, keeping whatever
/// the caller left unspecified.
pub(crate) async fn update_lifetime_priority(
    pool: &PgPool,
    id: &Uri,
    lifetime: Option<&Iso8601Duration>,
    priority: Option<SnapshotPriority>,
    new_expires_at: Option<DateTime<Utc>>,
) -> Result<u64, SnapshotError> {
    let statement = Query::update()
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
        .value(Snapshots::ModifiedAt, now())
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Move one preparing snapshot to its final status, recording the per-query
/// details of the run.
pub(crate) async fn update_status(
    pool: &PgPool,
    id: &Uri,
    status: SnapshotStatus,
    queries_details: Option<&[ExecutionResultDetails]>,
    temporal_queries_details: Option<&[ExecutionResultDetails]>,
) -> Result<u64, SnapshotError> {
    let statement = Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::Status, cast(Expr::val(<&str>::from(status)), PostgresType::SnapshotStatus))
        .value(Snapshots::QueriesDetails, Expr::val(queries_details.and_then(|d| serde_json::to_value(d).ok())))
        .value(
            Snapshots::TemporalQueriesDetails,
            Expr::val(temporal_queries_details.and_then(|d| serde_json::to_value(d).ok())),
        )
        .value(Snapshots::ModifiedAt, now())
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .and_where(status_is_preparing())
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
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
    let statement = Query::delete()
        .from_table(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Delete every snapshot whose expiry has passed `cutoff`.
pub(crate) async fn delete_expired_snapshots(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
    let statement = Query::delete()
        .from_table(Snapshots::Table)
        .and_where(Expr::col(Snapshots::ExpiresAt).lte(cutoff))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
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
    use crate::snapshot::lifecycle::{clone_snapshot_sql, next_preparing_sql, snapshot_create_sql};
    use sea_query::PostgresQueryBuilder;

    /// The `snapshot_status` column is a PostgreSQL enum: a text bind has no
    /// equality operator against it, so the preparing marker is cast.
    #[test]
    fn the_preparing_selection_casts_its_status_marker_to_the_enum() {
        let sql = next_preparing_sql().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"status\" = ('preparing'::snapshot_status)"), "{sql}");
        assert!(sql.contains("ORDER BY \"snapshot_priority\" DESC, \"created_at\" ASC LIMIT 1"), "{sql}");
    }

    #[test]
    fn the_create_casts_its_status_marker_and_stamps_no_clock() {
        let sql = snapshot_create_sql().expect("the fixed snapshot insert builds");
        assert!(sql.contains("$8::snapshot_status"), "{sql}");
        for marker in 1..=14 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$15"), "the create binds exactly fourteen values: {sql}");
        assert!(!sql.contains("snapshot_key"), "the surrogate is issued by the database, never written: {sql}");
    }

    #[test]
    fn the_clone_copies_the_control_record_and_stamps_fresh_clocks() {
        let sql = clone_snapshot_sql().expect("the fixed clone builds");
        assert!(sql.starts_with(r#"INSERT INTO "snapshots""#), "{sql}");
        assert!(sql.contains("SELECT $2,"), "the new id is the copied identifier: {sql}");
        assert!(sql.contains("NOW(), NOW()"), "created_at and modified_at are stamped fresh: {sql}");
        assert!(sql.contains(r#"WHERE "id" = ($1)"#), "{sql}");
        assert!(!sql.contains("snapshot_key"), "the clone receives its own surrogate from the database: {sql}");
    }
}
