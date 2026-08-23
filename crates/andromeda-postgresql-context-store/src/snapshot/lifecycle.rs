//! The statements over the `snapshots` control record: create, read, clone,
//! update and delete of one snapshot's own row.
//!
//! The child rows a snapshot captured are the concern of
//! [`crate::snapshot::content`]; the summaries read over them of
//! [`crate::snapshot::introspection`]. Everything here names a snapshot by its
//! URI: the surrogate `snapshot_key` is issued by the database and never
//! crosses this boundary.

use crate::snapshot::{mapper::row_to_snapshot, persistence_error};
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
use andromeda_snapshot::{
    error::SnapshotError,
    store::{InsertSnapshotParams, SnapshotPreparationJob},
};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::{Expr, ExprTrait, Func, Keyword, LockBehavior, LockType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Row, Transaction};
use url::Url;
use uuid::Uuid;

/// The stored-snapshot projection every read shares, decoding the status enum to
/// text under the column name the mapper reads it by.
fn apply_snapshot_projection(select: &mut SelectStatement) {
    select
        .column(Snapshots::Id)
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
fn snapshot_select() -> SelectStatement {
    let mut select = Query::select();
    apply_snapshot_projection(&mut select);
    select.from(Snapshots::Table);
    select
}

/// The columns a control record is written with, in the order the create and
/// the clone bind them; the surrogate key is issued by the database.
const fn control_columns() -> [Snapshots; 17] {
    [
        Snapshots::ContextSpaceKey,
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

/// The snapshot insert, its seventeen values bound by the caller's chain; the
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
            cast(external_placeholder(10), PostgresType::SnapshotStatus),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            external_placeholder(15),
            external_placeholder(16),
            external_placeholder(17),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Write one control record.
pub(crate) async fn create_snapshot(pool: &PgPool, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
    let mut tx = pool.begin().await.map_err(persistence_error)?;
    let context_space_key = create_snapshot_context_space(&mut tx).await?;
    let sql = snapshot_create_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(context_space_key.as_i64())
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
        .map_err(persistence_error)?;
    tx.commit().await.map_err(persistence_error)?;
    Ok(())
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
    let statement = Query::select()
        .column(Snapshots::ContextSpaceKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    row.map(|row| {
        let raw = row.try_get::<i64, _>("context_space_key").map_err(persistence_error)?;
        let key = ContextSpaceKey::try_from(raw).map_err(|error| SnapshotError::internal(error.to_string()))?;
        SnapshotTarget::new(ContextSpace::new(key, ContextSpaceKind::Snapshot), id.clone()).map_err(|error| SnapshotError::internal(error.to_string()))
    })
    .transpose()
}

/// `status = 'preparing'::snapshot_status`: the column is the PostgreSQL enum,
/// so the literal is cast to it rather than bound as text, which the enum has
/// no equality operator for.
fn status_is_preparing() -> SimpleExpr {
    Expr::col(Snapshots::Status).eq(cast(Expr::val(<&str>::from(SnapshotStatus::Preparing)), PostgresType::SnapshotStatus))
}

/// The statement selecting and row-locking the next snapshot to prepare.
///
/// `FOR UPDATE SKIP LOCKED` steps over a candidate another preparer already
/// holds, while the lease predicate permits safe reclamation after expiry.
fn next_preparing_sql() -> SelectStatement {
    let mut select = snapshot_select();
    select
        .and_where(status_is_preparing())
        .and_where(
            Expr::col(Snapshots::LeaseUntil)
                .is_null()
                .or(Expr::col(Snapshots::LeaseUntil).lte(Expr::current_timestamp())),
        )
        .order_by(Snapshots::SnapshotPriority, Order::Desc)
        .order_by(Snapshots::CreatedAt, Order::Asc)
        .limit(1)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    select
}

/// Stamp the durable preparation claim on one snapshot.
fn claim_snapshot_sql(id: &Uri, claimed_by: Uuid, lease_until: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::ClaimedBy, claimed_by)
        .value(Snapshots::ClaimedAt, now())
        .value(Snapshots::LeaseUntil, lease_until)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned()
}

/// Claim the next snapshot awaiting preparation: highest priority first, oldest
/// within a priority.
///
/// The select-and-claim runs in one transaction so the `FOR UPDATE SKIP LOCKED`
/// row lock is still held when the durable `claimed_at` stamp lands: the
/// on-create kick (`SnapshotService::prepare_now`) and the scheduled sweep can
/// both look for work at the same instant, and this is what stops them
/// executing the same snapshot's queries twice.
pub(crate) async fn next_preparing_snapshot(
    pool: &PgPool,
    claimed_by: Uuid,
    lease_until: DateTime<Utc>,
) -> Result<Option<SnapshotPreparationJob>, SnapshotError> {
    let mut tx = pool.begin().await.map_err(persistence_error)?;
    let (sql, values) = next_preparing_sql().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut *tx)
        .await
        .map_err(persistence_error)?;
    let Some(row) = row else {
        tx.rollback().await.map_err(persistence_error)?;
        return Ok(None);
    };
    let snapshot = row_to_snapshot(&row).map_err(persistence_error)?;
    let clone_source_id = row
        .try_get::<Option<String>, _>("clone_source_id")
        .map_err(persistence_error)?
        .map(|source| source.parse::<Uri>())
        .transpose()
        .map_err(|error| SnapshotError::internal(format!("Invalid clone source URI in snapshots table: {error}")))?;
    let query_context = row.try_get::<serde_json::Value, _>("query_context").map_err(persistence_error)?;
    let (claim_sql, claim_values) = claim_snapshot_sql(&snapshot.id, claimed_by, lease_until).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(claim_sql.as_str()), claim_values)
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
    tx.commit().await.map_err(persistence_error)?;
    Ok(Some(SnapshotPreparationJob {
        snapshot,
        clone_source_id,
        query_context,
    }))
}

/// Extend a claim held by this replica, returning false if ownership was lost.
pub(crate) async fn renew_preparation_lease(pool: &PgPool, id: &Uri, claimed_by: Uuid, lease_until: DateTime<Utc>) -> Result<bool, SnapshotError> {
    let statement = Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::LeaseUntil, lease_until)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .and_where(Expr::col(Snapshots::ClaimedBy).eq(claimed_by))
        .and_where(status_is_preparing())
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected() == 1)
}

/// Apply the mutable metadata of one snapshot, keeping whatever the caller
/// left unspecified and preserving any active preparation lease.
pub(crate) async fn update_snapshot_metadata(
    pool: &PgPool,
    id: &Uri,
    lifetime: Option<&Iso8601Duration>,
    priority: Option<SnapshotPriority>,
    endpoint: Option<Option<&Url>>,
    receiver_info: Option<Option<serde_json::Value>>,
    new_expires_at: Option<DateTime<Utc>>,
) -> Result<u64, SnapshotError> {
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
    if let Some(endpoint) = endpoint {
        statement.value(Snapshots::Endpoint, endpoint.map(ToString::to_string));
    }
    if let Some(receiver_info) = receiver_info {
        statement.value(Snapshots::ReceiverInfo, receiver_info);
    }
    statement.and_where(Expr::col(Snapshots::Id).eq(id.as_str()));
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
        .value(Snapshots::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(Snapshots::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(Snapshots::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
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
    let context_space = Query::select()
        .column(Snapshots::ContextSpaceKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let statement = Query::delete()
        .from_table(ContextSpaces::Table)
        .and_where(Expr::col(ContextSpaces::ContextSpaceKey).in_subquery(context_space))
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
    let expired_context_spaces = Query::select()
        .column(Snapshots::ContextSpaceKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::ExpiresAt).lte(cutoff))
        .to_owned();
    let statement = Query::delete()
        .from_table(ContextSpaces::Table)
        .and_where(Expr::col(ContextSpaces::ContextSpaceKey).in_subquery(expired_context_spaces))
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
    use crate::snapshot::lifecycle::{claim_snapshot_sql, next_preparing_sql, snapshot_create_sql};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use chrono::{TimeZone, Utc};
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    /// The `snapshot_status` column is a PostgreSQL enum: a text bind has no
    /// equality operator against it, so the preparing marker is cast.
    #[test]
    fn the_preparing_selection_casts_its_status_marker_to_the_enum() {
        let sql = next_preparing_sql().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"status\" = ('preparing'::snapshot_status)"), "{sql}");
        assert!(sql.contains("ORDER BY \"snapshot_priority\" DESC, \"created_at\" ASC LIMIT 1"), "{sql}");
    }

    /// Two preparers must never settle on the same control record: the
    /// selection excludes an already-claimed row and locks its candidate with
    /// `FOR UPDATE SKIP LOCKED` so a concurrent preparer skips it.
    #[test]
    fn the_preparing_selection_skips_claimed_and_locked_rows() {
        let sql = next_preparing_sql().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"lease_until\" IS NULL OR \"lease_until\" <= CURRENT_TIMESTAMP"), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
    }

    /// The durable claim stamps `claimed_at` on exactly the selected row.
    #[test]
    fn the_claim_stamps_claimed_at_on_the_selected_snapshot() {
        let id: Uri = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let worker = Uuid::from_u128(1);
        let lease_until = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 30).single().expect("valid lease instant");
        let sql = claim_snapshot_sql(&id, worker, lease_until).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("UPDATE \"snapshots\" SET \"claimed_by\" ="), "{sql}");
        assert!(sql.contains("\"claimed_at\" = NOW()"), "{sql}");
        assert!(sql.contains("\"lease_until\" ="), "{sql}");
        assert!(sql.contains("WHERE \"id\" = 'urn:ngsi-ld:Snapshot:1'"), "{sql}");
    }

    #[test]
    fn the_create_casts_its_status_marker_and_stamps_no_clock() {
        let sql = snapshot_create_sql().expect("the fixed snapshot insert builds");
        assert!(sql.contains("$9::snapshot_status"), "{sql}");
        for marker in 1..=16 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$17"), "the create binds exactly sixteen values: {sql}");
        assert!(sql.contains("context_space_key"), "the branch parent is explicit: {sql}");
        assert!(sql.contains("clone_source_id"), "the durable preparation kind is explicit: {sql}");
        assert!(
            !sql.contains("snapshot_key"),
            "the snapshot surrogate is issued by the database, never written: {sql}"
        );
    }
}
