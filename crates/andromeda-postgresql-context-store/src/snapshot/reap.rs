//! Reaping Snapshots: deletion, expiry, and the branch-schema teardown both
//! share with quota eviction.
//!
//! Reaping drops a Snapshot's branch schemas and deletes its control record,
//! which cascades the preparation and outbox rows. A quarantined Snapshot is
//! reaped like any other: nothing here reads its branch schema version.

use crate::snapshot::{branch_schema::drop_branch_schema, persistence_error};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_snapshot::error::SnapshotError;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, LockType, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use tokio::time::sleep;

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
/// one it does not have -- the published schema while it prepares, the staging
/// schema once published.
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
