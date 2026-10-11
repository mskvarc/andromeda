//! Reaping Snapshots: deletion, expiry, and the branch-schema teardown both
//! share with quota eviction.
//!
//! Reaping drops a Snapshot's branch schemas and deletes its control record,
//! which cascades the preparation and outbox rows. A quarantined Snapshot is
//! reaped like any other: nothing here reads its branch schema version.
//!
//! The schemas are dropped only when the Snapshot's branch lock is free. A
//! reap never waits for it: when PostgreSQL maintenance holds it, the control
//! record is still deleted, so the Snapshot is gone for every client, and the
//! schemas stay until an orphan sweep wins the lock. The sweep runs with the
//! expiry reaper and drops every branch schema without a control record, which
//! also covers a schema a crashed reap left behind. A branch schema is created
//! in the transaction that inserts its control record, so the sweep never sees
//! one whose record is still uncommitted.

use crate::{
    snapshot::{branch_lock::branch_lock_key, branch_schema::drop_branch_schema, persistence_error},
    store_fault::undecodable,
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::SNAPSHOT_SCHEMA_PATTERN;
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::{
    advisory_lock::try_advisory_xact_lock,
    cast::cast,
    operator::{regex_match, regex_substring},
    pg_type::PostgresType,
};
use andromeda_snapshot::error::SnapshotError;
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, LockType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use tokio::time::sleep;
use tracing::info;

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
pub(crate) async fn delete_snapshot(pool: &PgPool, id: &Identifier) -> Result<u64, SnapshotError> {
    retry_contended(async || delete_snapshot_once(pool, id).await).await
}

/// Run one branch-reaping transaction, re-running it from a fresh transaction
/// while a deadlock rolls it back, up to [`BRANCH_REAP_ATTEMPTS`] attempts.
async fn retry_contended(mut reap_once: impl AsyncFnMut() -> Result<u64, SnapshotError>) -> Result<u64, SnapshotError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match reap_once().await {
            Err(SnapshotError::Contended) if attempt < BRANCH_REAP_ATTEMPTS => sleep(BRANCH_REAP_BACKOFF).await,
            outcome => return outcome,
        }
    }
}

/// Delete one Snapshot's control record and branch schema in a single
/// transaction, returning the number of control rows removed.
async fn delete_snapshot_once(pool: &PgPool, id: &Identifier) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(|error| persistence_error(error))?;
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
        .map_err(|error| persistence_error(error))?
    else {
        transaction.rollback().await.map_err(|error| persistence_error(error))?;
        return Ok(0);
    };
    let snapshot_key = SnapshotKey::try_from(raw).map_err(|error| SnapshotError::from(undecodable(&error)))?;
    reap_branch(&mut transaction, snapshot_key).await?;
    let deleted = delete_control_record(&mut transaction, snapshot_key).await?;
    transaction.commit().await.map_err(|error| persistence_error(error))?;
    Ok(deleted)
}

/// Delete every snapshot whose expiry has passed `cutoff`, then sweep the
/// branch schemas no control record owns any more, returning the number of
/// expired Snapshots deleted.
pub(crate) async fn delete_expired_snapshots(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
    let deleted = retry_contended(async || delete_expired_snapshots_once(pool, cutoff).await).await?;
    let swept = retry_contended(async || sweep_orphan_branches_once(pool).await).await?;
    if swept > 0 {
        info!(
            swept_snapshots = swept,
            event = "snapshot_orphan_branches_dropped",
            "Snapshot branch schemas without a control record dropped"
        );
    }
    Ok(deleted)
}

/// Delete every Snapshot past `cutoff` and its branch schema in a single
/// transaction, returning the number of control rows removed.
async fn delete_expired_snapshots_once(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(|error| persistence_error(error))?;
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
        .map_err(|error| persistence_error(error))?;
    if rows.is_empty() {
        transaction.rollback().await.map_err(|error| persistence_error(error))?;
        return Ok(0);
    }
    let mut deleted = 0;
    for raw in rows {
        let snapshot_key = SnapshotKey::try_from(raw).map_err(|error| SnapshotError::from(undecodable(&error)))?;
        reap_branch(&mut transaction, snapshot_key).await?;
        deleted += delete_control_record(&mut transaction, snapshot_key).await?;
    }
    transaction.commit().await.map_err(|error| persistence_error(error))?;
    Ok(deleted)
}

/// Drop the branch schemas of every Snapshot without a control record whose
/// branch lock is free, in one transaction, returning how many Snapshots'
/// schemas were dropped. A schema whose lock maintenance holds stays for the
/// next sweep.
async fn sweep_orphan_branches_once(pool: &PgPool) -> Result<u64, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(|error| persistence_error(error))?;
    let (sql, values) = orphan_branch_keys_statement().build_sqlx(PostgresQueryBuilder);
    let keys = sqlx::query_scalar_with::<_, i64, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    let mut swept = 0;
    for raw in keys {
        let snapshot_key = SnapshotKey::try_from(raw).map_err(|error| SnapshotError::from(undecodable(&error)))?;
        if try_branch_lock(&mut transaction, snapshot_key).await? {
            drop_branch_schemas(&mut transaction, snapshot_key).await?;
            swept += 1;
        }
    }
    transaction.commit().await.map_err(|error| persistence_error(error))?;
    Ok(swept)
}

/// The Snapshot key of every published or staging branch schema whose
/// Snapshot has no control record.
pub(crate) fn orphan_branch_keys_statement() -> SelectStatement {
    let namespace = || Expr::col(Alias::new("nspname"));
    let schema_key = || cast(regex_substring(namespace(), Expr::val(SNAPSHOT_SCHEMA_PATTERN)), PostgresType::Bigint);
    let control_record = Query::select()
        .expr(Expr::val(1))
        .from(Snapshots::Table)
        .and_where(Expr::col((Snapshots::Table, Snapshots::SnapshotKey)).eq(schema_key()))
        .to_owned();
    Query::select()
        .distinct()
        .expr_as(schema_key(), Snapshots::SnapshotKey)
        .from(Alias::new("pg_namespace"))
        .and_where(regex_match(namespace(), Expr::val(SNAPSHOT_SCHEMA_PATTERN)))
        .and_where(Expr::exists(control_record).not())
        .to_owned()
}

/// Drop both possible branch schemas of one Snapshot when its branch lock is
/// free. When PostgreSQL maintenance holds the lock the schemas stay for the
/// orphan sweep; the caller deletes the control record either way.
pub(crate) async fn reap_branch(transaction: &mut Transaction<'_, Postgres>, snapshot_key: SnapshotKey) -> Result<(), SnapshotError> {
    if try_branch_lock(transaction, snapshot_key).await? {
        drop_branch_schemas(transaction, snapshot_key).await
    } else {
        info!(
            snapshot_key = snapshot_key.as_i64(),
            event = "snapshot_branch_drop_deferred",
            "Snapshot branch is in maintenance; its schema is dropped by a later orphan sweep"
        );
        Ok(())
    }
}

/// Try the Snapshot's branch lock for the rest of the transaction, without
/// waiting for it.
async fn try_branch_lock(transaction: &mut Transaction<'_, Postgres>, snapshot_key: SnapshotKey) -> Result<bool, SnapshotError> {
    let (sql, values) = branch_lock_statement(snapshot_key).build_sqlx(PostgresQueryBuilder);
    sqlx::query_scalar_with::<_, bool, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error(error))
}

/// The statement a reap transaction tries its branch lock with.
fn branch_lock_statement(snapshot_key: SnapshotKey) -> SelectStatement {
    Query::select().expr(try_advisory_xact_lock(branch_lock_key(snapshot_key))).to_owned()
}

/// Drop both possible branch schemas of one Snapshot; `IF EXISTS` covers the
/// one it does not have -- the published schema while it prepares, the staging
/// schema once published.
async fn drop_branch_schemas(transaction: &mut Transaction<'_, Postgres>, snapshot_key: SnapshotKey) -> Result<(), SnapshotError> {
    drop_branch_schema(transaction, snapshot_key, SnapshotBranch::Published)
        .await
        .map_err(|error| persistence_error(error))?;
    drop_branch_schema(transaction, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(|error| persistence_error(error))?;
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
        .map_err(|error| persistence_error(error))?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use crate::snapshot::reap::{branch_lock_statement, orphan_branch_keys_statement};
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn branch_reap_tries_the_branch_lock_and_never_waits() {
        let sql = branch_lock_statement(SnapshotKey::try_from(42).expect("positive snapshot key")).to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_try_advisory_xact_lock(hashtext('andromeda.snapshot.branch.42'))");
        assert!(!sql.contains("pg_advisory_xact_lock"), "{sql}");
        assert!(!sql.contains("pg_advisory_lock"), "{sql}");
    }

    #[test]
    fn orphan_sweep_selects_branch_schemas_without_a_control_record() {
        let sql = orphan_branch_keys_statement().to_string(PostgresQueryBuilder);
        let key = r#"substring("nspname", '^snap_([1-9][0-9]*)(_stg)?$')::bigint"#;
        assert!(
            sql.starts_with(&format!(r#"SELECT DISTINCT {key} AS "snapshot_key" FROM "pg_namespace""#)),
            "{sql}"
        );
        assert!(
            sql.contains(r#""nspname" ~ '^snap_([1-9][0-9]*)(_stg)?$'"#),
            "published and staging names: {sql}"
        );
        assert!(
            sql.contains(&format!(r#"NOT EXISTS(SELECT 1 FROM "snapshots" WHERE "snapshots"."snapshot_key" = ({key}))"#)),
            "{sql}"
        );
    }
}
