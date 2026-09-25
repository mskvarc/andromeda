//! Durable Snapshot preparation claiming, lease renewal, and staging reset.

use crate::{
    migration_set::branch::BranchSchemaVersion,
    snapshot::{
        branch_schema::truncate_branch_schema,
        branch_version::{branch_is_current, current_version},
        mapper::row_to_snapshot,
        persistence_error,
        record::{snapshot_select, status_is_preparing},
    },
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::clock::now;
use andromeda_snapshot::{error::SnapshotError, store::SnapshotPreparationJob};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, LockBehavior, LockType, Order, PostgresQueryBuilder, Query, SelectStatement, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};

/// Select and row-lock the next Snapshot eligible for preparation, carrying its
/// surrogate key so the claim can reset the staging branch.
///
/// A quarantined Snapshot is never claimed: its staging branch is behind this
/// binary's branch relations, so a capture into it could not be trusted.
fn next_preparing_statement(current: BranchSchemaVersion) -> SelectStatement {
    let mut select = snapshot_select();
    select
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .and_where(status_is_preparing())
        .and_where(branch_is_current(current))
        .and_where(
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::LeaseUntil))
                .is_null()
                .or(Expr::col((SnapshotPreparation::Table, SnapshotPreparation::LeaseUntil)).lte(Expr::current_timestamp())),
        )
        .order_by((Snapshots::Table, Snapshots::SnapshotPriority), Order::Desc)
        .order_by((Snapshots::Table, Snapshots::CreatedAt), Order::Asc)
        .limit(1)
        .lock_with_tables_behavior(LockType::Update, [SnapshotPreparation::Table], LockBehavior::SkipLocked);
    select
}

/// Stamp the durable preparation claim on one Snapshot's preparation row.
fn claim_statement(snapshot_key: SnapshotKey, claimed_by: LeaseHolder, lease_until: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::ClaimedBy, claimed_by.as_uuid())
        .value(SnapshotPreparation::ClaimedAt, now())
        .value(SnapshotPreparation::LeaseUntil, lease_until)
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(snapshot_key.as_i64()))
        .to_owned()
}

/// Claim the highest-priority, oldest eligible Snapshot in one locked
/// transaction, resetting its staging branch before the worker re-runs it.
pub(crate) async fn next_preparing_snapshot(
    pool: &PgPool,
    claimed_by: LeaseHolder,
    lease_until: DateTime<Utc>,
) -> Result<Option<SnapshotPreparationJob>, SnapshotError> {
    let current = current_version()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let (sql, values) = next_preparing_statement(current).build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let Some(row) = row else {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(None);
    };
    let snapshot = row_to_snapshot(&row).map_err(persistence_error)?;
    let snapshot_key =
        SnapshotKey::try_from(row.try_get::<i64, _>("snapshot_key").map_err(persistence_error)?).map_err(|error| SnapshotError::internal(error.to_string()))?;
    let clone_source_id = row
        .try_get::<Option<String>, _>("clone_source_id")
        .map_err(persistence_error)?
        .map(|source| source.parse::<Uri>())
        .transpose()
        .map_err(|error| SnapshotError::internal(format!("Invalid clone source URI in snapshots table: {error}")))?;
    let query_context = row.try_get::<serde_json::Value, _>("query_context").map_err(persistence_error)?;
    let (sql, values) = claim_statement(snapshot_key, claimed_by, lease_until).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    truncate_branch_schema(&mut transaction, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(Some(SnapshotPreparationJob {
        snapshot,
        clone_source_id,
        query_context,
    }))
}

/// Extend a claim held by this replica, returning false if ownership was lost.
pub(crate) async fn renew_preparation_lease(pool: &PgPool, id: &Uri, claimed_by: LeaseHolder, lease_until: DateTime<Utc>) -> Result<bool, SnapshotError> {
    let key_subquery = Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let statement = Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::LeaseUntil, lease_until)
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).in_subquery(key_subquery))
        .and_where(Expr::col(SnapshotPreparation::ClaimedBy).eq(claimed_by.as_uuid()))
        .and_where(status_is_preparing())
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::branch::BranchSchemaVersion,
        snapshot::preparation_claim::{claim_statement, next_preparing_statement},
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use chrono::{TimeZone, Utc};
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    #[test]
    fn the_preparing_selection_orders_by_priority_then_age_and_skips_leased_rows() {
        let current = BranchSchemaVersion::try_from(21_i64).expect("a positive version");
        let sql = next_preparing_statement(current).to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"snapshot_preparation\".\"status\" = 1"), "{sql}");
        assert!(
            sql.contains("\"snapshot_preparation\".\"branch_schema_version\" >= 21"),
            "a quarantined Snapshot is never claimed: {sql}"
        );
        assert!(
            sql.contains("ORDER BY \"snapshots\".\"snapshot_priority\" DESC, \"snapshots\".\"created_at\" ASC LIMIT 1"),
            "{sql}"
        );
        assert!(sql.contains("FOR UPDATE OF \"snapshot_preparation\" SKIP LOCKED"), "{sql}");
    }

    #[test]
    fn the_claim_stamps_the_lease_on_the_preparation_row_by_key() {
        let key = SnapshotKey::try_from(7).expect("positive key");
        let worker = LeaseHolder::from(Uuid::from_u128(1));
        let lease_until = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 30).single().expect("valid lease instant");
        let sql = claim_statement(key, worker, lease_until).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("UPDATE \"snapshot_preparation\" SET \"claimed_by\" ="), "{sql}");
        assert!(sql.contains("\"claimed_at\" = NOW()"), "{sql}");
        assert!(sql.contains("WHERE \"snapshot_key\" = 7"), "{sql}");
    }
}
