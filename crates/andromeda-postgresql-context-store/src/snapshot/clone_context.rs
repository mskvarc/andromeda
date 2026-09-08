//! Atomic point-in-time copying of one Snapshot context branch into another.

use crate::{
    context_space::delete_context_space_statement,
    snapshot::{clone_statements::clone_statements, persistence_error, status_outbox::enqueue_terminal_notification},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, SnapshotStatus};
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::{cast::cast, clock::now, pg_type::PostgresType, transaction::repeatable_read};
use andromeda_snapshot::error::SnapshotError;
use sea_query::{Expr, ExprTrait, Iden, Keyword, LockType, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone, Copy, Iden)]
#[iden = "source"]
/// Source Snapshot control-row alias.
struct Source;

#[derive(Clone, Copy, Iden)]
#[iden = "target"]
/// Target Snapshot control-row alias.
struct Target;

/// Copy every current and temporal relation in one repeatable-read transaction.
pub(crate) async fn promote_context_clone(pool: &PgPool, source_id: &Uri, target_id: &Uri, claimed_by: Uuid) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(sqlx::AssertSqlSafe(repeatable_read()))
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let source_and_staging = Query::select()
        .column((Source, Snapshots::ContextSpaceKey))
        .column((Target, Snapshots::StagingContextSpaceKey))
        .from_as(Snapshots::Table, Source)
        .from_as(Snapshots::Table, Target)
        .and_where(Expr::col((Source, Snapshots::Id)).eq(source_id.as_str()))
        .and_where(Expr::col((Target, Snapshots::Id)).eq(target_id.as_str()))
        .and_where(Expr::col((Target, Snapshots::ClaimedBy)).eq(claimed_by))
        .and_where(preparing(Target))
        .to_owned();
    let (sql, values) = source_and_staging.build_sqlx(PostgresQueryBuilder);
    let (source_key, staging_key): (i64, i64) = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;

    for statement in clone_statements(source_key, staging_key)? {
        execute(&mut transaction, statement).await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    promote_control_row(pool, target_id, claimed_by, staging_key).await
}

/// Publish one query capture from its private context space in a single transaction.
pub(crate) async fn promote_staged_capture(
    pool: &PgPool,
    id: &Uri,
    claimed_by: Uuid,
    status: SnapshotStatus,
    queries_details: Option<&[ExecutionResultDetails]>,
    temporal_queries_details: Option<&[ExecutionResultDetails]>,
) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let locked = Query::select()
        .columns([Snapshots::ContextSpaceKey, Snapshots::StagingContextSpaceKey])
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .and_where(Expr::col(Snapshots::ClaimedBy).eq(claimed_by))
        .and_where(preparing(Snapshots::Table))
        .lock(LockType::Update)
        .to_owned();
    let (sql, values) = locked.build_sqlx(PostgresQueryBuilder);
    let row: (i64, Option<i64>) = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let staging_key = row
        .1
        .ok_or_else(|| SnapshotError::internal(format!("Snapshot {id} has no staging context space")))?;

    let published_key = if status == SnapshotStatus::Failure { row.0 } else { staging_key };
    let discarded_key = if status == SnapshotStatus::Failure { staging_key } else { row.0 };
    let statement = Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::ContextSpaceKey, published_key)
        .value(Snapshots::Status, cast(Expr::val(<&str>::from(status)), PostgresType::SnapshotStatus))
        .value(Snapshots::QueriesDetails, queries_details.map(serde_json::to_value).transpose()?)
        .value(
            Snapshots::TemporalQueriesDetails,
            temporal_queries_details.map(serde_json::to_value).transpose()?,
        )
        .value(Snapshots::ModifiedAt, now())
        .value(Snapshots::StagingContextSpaceKey, Keyword::Null)
        .value(Snapshots::ClaimedBy, Keyword::Null)
        .value(Snapshots::ClaimedAt, Keyword::Null)
        .value(Snapshots::LeaseUntil, Keyword::Null)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .and_where(Expr::col(Snapshots::ClaimedBy).eq(claimed_by))
        .and_where(preparing(Snapshots::Table))
        .to_owned();
    let rows_affected = execute(&mut transaction, statement).await?;
    if rows_affected != 1 {
        return Err(SnapshotError::internal(format!("Snapshot {id} is no longer preparing")));
    }
    enqueue_terminal_notification(&mut transaction, id).await?;
    execute(&mut transaction, delete_context_space_statement(discarded_key)).await?;
    transaction.commit().await.map_err(persistence_error)
}

/// Atomically publish the staged clone and discard its original empty branch.
async fn promote_control_row(pool: &PgPool, target_id: &Uri, claimed_by: Uuid, staging_key: i64) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let locked = Query::select()
        .column(Snapshots::ContextSpaceKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(target_id.as_str()))
        .and_where(Expr::col(Snapshots::ClaimedBy).eq(claimed_by))
        .and_where(Expr::col(Snapshots::StagingContextSpaceKey).eq(staging_key))
        .and_where(preparing(Snapshots::Table))
        .lock(LockType::Update)
        .to_owned();
    let (sql, values) = locked.build_sqlx(PostgresQueryBuilder);
    let original_key: i64 = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let statement = Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::ContextSpaceKey, staging_key)
        .value(Snapshots::StagingContextSpaceKey, Keyword::Null)
        .value(
            Snapshots::Status,
            cast(Expr::val(<&str>::from(SnapshotStatus::Success)), PostgresType::SnapshotStatus),
        )
        .value(Snapshots::ModifiedAt, now())
        .value(Snapshots::ClaimedBy, Keyword::Null)
        .value(Snapshots::ClaimedAt, Keyword::Null)
        .value(Snapshots::LeaseUntil, Keyword::Null)
        .and_where(Expr::col(Snapshots::Id).eq(target_id.as_str()))
        .and_where(Expr::col(Snapshots::ClaimedBy).eq(claimed_by))
        .and_where(preparing(Snapshots::Table))
        .to_owned();
    let rows_affected = execute(&mut transaction, statement).await?;
    if rows_affected != 1 {
        return Err(SnapshotError::internal(format!("Clone target {target_id} is no longer preparing")));
    }
    enqueue_terminal_notification(&mut transaction, target_id).await?;
    execute(&mut transaction, delete_context_space_statement(original_key)).await?;
    transaction.commit().await.map_err(persistence_error)
}

/// Match a Snapshot control row whose typed status is `preparing`.
fn preparing<T>(table: T) -> SimpleExpr
where
    T: Iden + 'static,
{
    Expr::col((table, Snapshots::Status)).eq(cast(Expr::val(<&str>::from(SnapshotStatus::Preparing)), PostgresType::SnapshotStatus))
}

/// Execute one generated statement inside the clone-promotion transaction.
async fn execute(transaction: &mut Transaction<'_, Postgres>, statement: impl SqlxBinder) -> Result<u64, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}
