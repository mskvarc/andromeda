//! Durable Snapshot preparation claiming, lease renewal, and stale-stage cleanup.

use crate::snapshot::{
    lifecycle::{snapshot_select, status_is_preparing},
    mapper::row_to_snapshot,
    persistence_error,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::{
    current::Entities,
    notification::NotificationEvents,
    queue::{PendingSideEffects, TemporalAppendIntents},
    snapshot::Snapshots,
    subscriptions::Subscriptions,
    temporal::{AttributeHistory, ScopeHistory, TemporalEntities},
};
use andromeda_postgresql_seaquery::clock::now;
use andromeda_snapshot::{error::SnapshotError, store::SnapshotPreparationJob};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, LockBehavior, LockType, Order, PostgresQueryBuilder, Query, SelectStatement, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

/// Select and row-lock the next Snapshot eligible for preparation.
fn next_preparing_statement() -> SelectStatement {
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

/// Stamp the durable preparation claim on one Snapshot.
fn claim_statement(id: &Uri, claimed_by: Uuid, lease_until: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(Snapshots::Table)
        .value(Snapshots::ClaimedBy, claimed_by)
        .value(Snapshots::ClaimedAt, now())
        .value(Snapshots::LeaseUntil, lease_until)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned()
}

/// Claim the highest-priority, oldest eligible Snapshot in one locked transaction.
pub(crate) async fn next_preparing_snapshot(
    pool: &PgPool,
    claimed_by: Uuid,
    lease_until: DateTime<Utc>,
) -> Result<Option<SnapshotPreparationJob>, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let (sql, values) = next_preparing_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let Some(row) = row else {
        transaction.rollback().await.map_err(persistence_error)?;
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
    let staging_context_space_key = row.try_get::<Option<i64>, _>("staging_context_space_key").map_err(persistence_error)?;
    let (sql, values) = claim_statement(&snapshot.id, claimed_by, lease_until).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if let Some(staging_key) = staging_context_space_key {
        clear_staging_context(&mut transaction, staging_key).await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(Some(SnapshotPreparationJob {
        snapshot,
        clone_source_id,
        query_context,
    }))
}

/// Remove branch rows left by an expired preparation lease before re-execution.
async fn clear_staging_context(transaction: &mut Transaction<'_, Postgres>, context_space_key: i64) -> Result<(), SnapshotError> {
    macro_rules! clear_relation {
        ($table:expr, $column:expr) => {{
            let statement = Query::delete()
                .from_table($table)
                .and_where(Expr::col($column).eq(context_space_key))
                .to_owned();
            execute(transaction, statement).await?;
        }};
    }
    clear_relation!(NotificationEvents::Table, NotificationEvents::ContextSpaceKey);
    clear_relation!(Subscriptions::Table, Subscriptions::ContextSpaceKey);
    clear_relation!(PendingSideEffects::Table, PendingSideEffects::ContextSpaceKey);
    clear_relation!(TemporalAppendIntents::Table, TemporalAppendIntents::ContextSpaceKey);
    clear_relation!(AttributeHistory::Table, AttributeHistory::ContextSpaceKey);
    clear_relation!(ScopeHistory::Table, ScopeHistory::ContextSpaceKey);
    clear_relation!(TemporalEntities::Table, TemporalEntities::ContextSpaceKey);
    clear_relation!(Entities::Table, Entities::ContextSpaceKey);
    Ok(())
}

/// Execute one generated cleanup statement inside the claim transaction.
async fn execute(transaction: &mut Transaction<'_, Postgres>, statement: impl SqlxBinder) -> Result<u64, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
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

#[cfg(test)]
mod tests {
    use crate::snapshot::preparation_claim::{claim_statement, next_preparing_statement};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use chrono::{TimeZone, Utc};
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    #[test]
    fn the_preparing_selection_casts_its_status_marker_to_the_enum() {
        let sql = next_preparing_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"status\" = ('preparing'::snapshot_status)"), "{sql}");
        assert!(sql.contains("ORDER BY \"snapshot_priority\" DESC, \"created_at\" ASC LIMIT 1"), "{sql}");
    }

    #[test]
    fn the_preparing_selection_skips_claimed_and_locked_rows() {
        let sql = next_preparing_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"lease_until\" IS NULL OR \"lease_until\" <= CURRENT_TIMESTAMP"), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
    }

    #[test]
    fn the_claim_stamps_claimed_at_on_the_selected_snapshot() {
        let id: Uri = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let worker = Uuid::from_u128(1);
        let lease_until = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 30).single().expect("valid lease instant");
        let sql = claim_statement(&id, worker, lease_until).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("UPDATE \"snapshots\" SET \"claimed_by\" ="), "{sql}");
        assert!(sql.contains("\"claimed_at\" = NOW()"), "{sql}");
        assert!(sql.contains("\"lease_until\" ="), "{sql}");
        assert!(sql.contains("WHERE \"id\" = 'urn:ngsi-ld:Snapshot:1'"), "{sql}");
    }
}
