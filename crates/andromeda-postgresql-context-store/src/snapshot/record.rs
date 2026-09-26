//! Reading and advancing a Snapshot's control record and preparation state.
//!
//! A Snapshot is two rows -- the immutable `snapshots` control record and the
//! mutable `snapshot_preparation` lease row, sharing `snapshot_key` -- plus its
//! own branch schema named after that key. Everything here names a Snapshot by
//! its URI; the surrogate `snapshot_key` is issued by the database and used only
//! to name the branch schema and to key the preparation row.

use crate::snapshot::{
    branch_schema::{promote_staging_schema, publish_empty_branch},
    mapper::row_to_snapshot,
    metadata_change::MetadataChange,
    persistence_error,
    status_code::stored_status,
    status_outbox::enqueue_terminal_notification,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
use andromeda_postgresql_context_query::snapshot_purge::build_match_query;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::clock::now;
use andromeda_snapshot::error::SnapshotError;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::{Expr, ExprTrait, Func, JoinType, Keyword, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Row, Transaction};
use url::Url;

/// The stored-snapshot projection every read shares, over the control record
/// joined to its preparation row.
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
        .column((SnapshotPreparation::Table, SnapshotPreparation::Status))
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

/// `status = <preparing>` on the preparation row, the code the claim index's
/// predicate names.
pub(crate) fn status_is_preparing() -> SimpleExpr {
    Expr::col((SnapshotPreparation::Table, SnapshotPreparation::Status)).eq(SnapshotStatusCode::Preparing.code())
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
/// details and publishing its staging branch -- emptied first on a failure, so
/// the failed Snapshot stays addressable over an empty dataset.
pub(crate) async fn update_status(
    pool: &PgPool,
    id: &Uri,
    claimed_by: LeaseHolder,
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
        .value(SnapshotPreparation::Status, stored_status(status).code())
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
        .and_where(Expr::col(SnapshotPreparation::ClaimedBy).eq(claimed_by.as_uuid()))
        .and_where(status_is_preparing())
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if result.rows_affected() == 1 {
        enqueue_terminal_notification(&mut transaction, id).await?;
        let published = if status == SnapshotStatus::Failure {
            publish_empty_branch(&mut transaction, snapshot_key).await
        } else {
            promote_staging_schema(&mut transaction, snapshot_key).await
        };
        published.map_err(persistence_error)?;
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
    use crate::snapshot::record::{snapshot_select, status_is_preparing};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn the_read_projection_joins_the_preparation_row_and_reads_the_status_code() {
        let sql = snapshot_select().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("FROM \"snapshots\" JOIN \"snapshot_preparation\" ON \"snapshot_preparation\".\"snapshot_key\" = \"snapshots\".\"snapshot_key\""),
            "{sql}"
        );
        assert!(sql.contains("\"snapshot_preparation\".\"status\","), "{sql}");
        assert!(!sql.contains("::text"), "{sql}");
    }

    #[test]
    fn the_preparing_predicate_names_the_preparing_code() {
        let sql = Query::select().expr(status_is_preparing()).to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT \"snapshot_preparation\".\"status\" = 1");
    }
}
