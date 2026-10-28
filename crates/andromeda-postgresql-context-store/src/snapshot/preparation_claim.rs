//! Durable Snapshot preparation claiming, lease renewal, restart and release,
//! the staging reset a claim or restart performs, and the `FOR KEY SHARE` fence
//! a clone copy holds on its claimed preparation.

use crate::{
    snapshot::{
        branch_schema::truncate_branch_schema,
        branch_version::current_version,
        mapper::{row_query_context, row_to_snapshot},
        record::{snapshot_select, status_is_preparing},
    },
    store_fault::{persistence_error, undecodable},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
use andromeda_postgresql_schema::{
    branch_schema_version::BranchSchemaVersion,
    snapshot::{SnapshotPreparation, Snapshots, branch_is_current},
};
use andromeda_postgresql_seaquery::{clock::now, statement::Statement};
use andromeda_snapshot::{error::SnapshotError, store::SnapshotPreparationJob};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, JoinType, Keyword, LockBehavior, LockType, Order, Query, SelectStatement, SimpleExpr, UpdateStatement};
use sqlx::{PgPool, Postgres, Row, Transaction};

/// The alias the clone-source lookup reads the source's control record under.
const CLONE_SOURCE: &str = "clone_source";
/// The alias the clone-source lookup reads the source's preparation row under.
const CLONE_SOURCE_PREPARATION: &str = "clone_source_preparation";

/// `SELECT 1` of the source of the outer Snapshot's clone job while that source
/// is still preparing; empty for a capture job and for a source that ended or
/// is gone.
fn clone_source_preparing() -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from_as(Snapshots::Table, Alias::new(CLONE_SOURCE))
        .join_as(
            JoinType::Join,
            SnapshotPreparation::Table,
            Alias::new(CLONE_SOURCE_PREPARATION),
            Expr::col((Alias::new(CLONE_SOURCE_PREPARATION), SnapshotPreparation::SnapshotKey)).equals((Alias::new(CLONE_SOURCE), Snapshots::SnapshotKey)),
        )
        .and_where(Expr::col((Alias::new(CLONE_SOURCE), Snapshots::Id)).equals((Snapshots::Table, Snapshots::CloneSourceId)))
        .and_where(Expr::col((Alias::new(CLONE_SOURCE_PREPARATION), SnapshotPreparation::Status)).eq(SnapshotStatusCode::Preparing.code()))
        .to_owned()
}

/// Select and row-lock the next Snapshot eligible for preparation, carrying its
/// surrogate key so the claim can reset the staging branch.
///
/// A quarantined Snapshot is never claimed: its staging branch is behind this
/// binary's branch relations, so a capture into it could not be trusted. A
/// clone is not claimed while its source is still preparing: clause 5.16.2.4
/// copies all data of the Snapshot it clones, which does not exist before the
/// source's preparation ends. The pass that ends the source claims again, so
/// the clone waits on nothing but the source. A clone whose source is gone is
/// claimed, and its copy fails.
fn next_preparing_statement(current: BranchSchemaVersion) -> SelectStatement {
    let mut select = snapshot_select();
    select
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .and_where(status_is_preparing())
        .and_where(branch_is_current(current))
        .and_where(Expr::exists(clone_source_preparing()).not())
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
    let mut transaction = pool.begin().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    let row = next_preparing_statement(current)
        .to_query()
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    let Some(row) = row else {
        transaction.rollback().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
        return Ok(None);
    };
    let snapshot = row_to_snapshot(&row).map_err(|error| persistence_error::<SnapshotError>(error))?;
    let snapshot_key = SnapshotKey::try_from(
        row.try_get::<i64, _>("snapshot_key")
            .map_err(|error| persistence_error::<SnapshotError>(error))?,
    )
    .map_err(|error| SnapshotError::from(undecodable(&error)))?;
    let clone_source_id = row
        .try_get::<Option<String>, _>("clone_source_id")
        .map_err(|error| persistence_error::<SnapshotError>(error))?
        .map(|source| source.parse::<Identifier>())
        .transpose()
        .map_err(|error| SnapshotError::from(undecodable(&error)))?;
    let query_context = row_query_context(&row).map_err(|error| persistence_error::<SnapshotError>(error))?;
    claim_statement(snapshot_key, claimed_by, lease_until)
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    truncate_branch_schema(&mut transaction, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(Some(SnapshotPreparationJob {
        snapshot,
        clone_source_id,
        query_context,
    }))
}

/// `SELECT snapshot_key FROM snapshots WHERE id = ?`, for a preparation-row
/// statement that reaches its row through the control record's URI.
fn key_of(id: &Identifier) -> SelectStatement {
    Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned()
}

/// Extend a claim held by this replica, returning false if ownership was lost.
pub(crate) async fn renew_preparation_lease(
    pool: &PgPool,
    id: &Identifier,
    claimed_by: LeaseHolder,
    lease_until: DateTime<Utc>,
) -> Result<bool, SnapshotError> {
    let statement = Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::LeaseUntil, lease_until)
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).in_subquery(key_of(id)))
        .and_where(Expr::col(SnapshotPreparation::ClaimedBy).eq(claimed_by.as_uuid()))
        .and_where(status_is_preparing())
        .to_owned();
    let result = statement
        .to_query()
        .execute(pool)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(result.rows_affected() == 1)
}

/// Select the key of one Snapshot whose preparation row is preparing and
/// claimed by `claimed_by`, reading the control record and the preparation row.
fn claimed_preparation_select(id: &Identifier, claimed_by: LeaseHolder) -> SelectStatement {
    Query::select()
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .from(Snapshots::Table)
        .join(
            JoinType::Join,
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        )
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(id.as_str()))
        .and_where(Expr::col((SnapshotPreparation::Table, SnapshotPreparation::ClaimedBy)).eq(claimed_by.as_uuid()))
        .and_where(status_is_preparing())
        .to_owned()
}

/// Select, and row-lock for a lifecycle transition, the preparing preparation
/// row of one Snapshot claimed by `claimed_by`.
fn claimed_preparation_statement(id: &Identifier, claimed_by: LeaseHolder) -> SelectStatement {
    claimed_preparation_select(id, claimed_by).lock(LockType::Update).to_owned()
}

/// Select, and fence for a clone copy, the preparing preparation row of one
/// Snapshot claimed by `claimed_by`.
///
/// `FOR KEY SHARE` on both rows conflicts only with `FOR UPDATE`, `DELETE` and
/// key updates. A lease renewal, a last-used touch and a metadata change update
/// non-key columns and proceed beside the copy; another worker's claim skips
/// the row, so the claim cannot change hands mid-copy; and every reaping path
/// takes `FOR UPDATE` on the control record before it drops a schema, so it
/// waits for the copy instead of dropping relations the copy is writing.
fn shared_claim_statement(id: &Identifier, claimed_by: LeaseHolder) -> SelectStatement {
    claimed_preparation_select(id, claimed_by).lock(LockType::KeyShare).to_owned()
}

/// Resolve, and row-lock for a lifecycle transition, the preparing preparation
/// row of one Snapshot claimed by `claimed_by`; `None` when the claim is no
/// longer held or the Snapshot has left `preparing`.
pub(crate) async fn lock_claimed_preparation(
    transaction: &mut Transaction<'_, Postgres>,
    id: &Identifier,
    claimed_by: LeaseHolder,
) -> Result<Option<SnapshotKey>, SnapshotError> {
    claimed_key(transaction, &claimed_preparation_statement(id, claimed_by)).await
}

/// Resolve, and fence for the length of a clone copy, the preparing preparation
/// row of one Snapshot claimed by `claimed_by`; `None` when the claim is no
/// longer held or the Snapshot has left `preparing`.
pub(crate) async fn share_claimed_preparation(
    transaction: &mut Transaction<'_, Postgres>,
    id: &Identifier,
    claimed_by: LeaseHolder,
) -> Result<Option<SnapshotKey>, SnapshotError> {
    claimed_key(transaction, &shared_claim_statement(id, claimed_by)).await
}

/// Run one claimed-preparation lookup, decoding the key it finds.
async fn claimed_key(transaction: &mut Transaction<'_, Postgres>, statement: &SelectStatement) -> Result<Option<SnapshotKey>, SnapshotError> {
    statement
        .to_query_scalar::<i64>()
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?
        .map(|raw| SnapshotKey::try_from(raw).map_err(|error| SnapshotError::from(undecodable(&error))))
        .transpose()
}

/// Extend the lease on one Snapshot's preparation row.
fn lease_statement(snapshot_key: SnapshotKey, lease_until: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::LeaseUntil, lease_until)
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(snapshot_key.as_i64()))
        .to_owned()
}

/// Restart a preparation still claimed by this replica: extend its lease and
/// empty its staging branch in one transaction, so the next attempt captures
/// into a branch no earlier attempt wrote. Returns false when the claim was
/// lost or the Snapshot left `preparing`.
pub(crate) async fn restart_preparation(pool: &PgPool, id: &Identifier, claimed_by: LeaseHolder, lease_until: DateTime<Utc>) -> Result<bool, SnapshotError> {
    let mut transaction = pool.begin().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    let Some(snapshot_key) = lock_claimed_preparation(&mut transaction, id, claimed_by).await? else {
        transaction.rollback().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
        return Ok(false);
    };
    lease_statement(snapshot_key, lease_until)
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    truncate_branch_schema(&mut transaction, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(true)
}

/// Clear the claim this replica holds on a still-preparing Snapshot.
fn release_statement(id: &Identifier, claimed_by: LeaseHolder) -> UpdateStatement {
    Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(SnapshotPreparation::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(SnapshotPreparation::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).in_subquery(key_of(id)))
        .and_where(Expr::col(SnapshotPreparation::ClaimedBy).eq(claimed_by.as_uuid()))
        .and_where(status_is_preparing())
        .to_owned()
}

/// Give up the claim this replica holds, so any worker's next pass claims the
/// Snapshot without waiting out the lease; a claim already lost is left alone.
pub(crate) async fn release_preparation(pool: &PgPool, id: &Identifier, claimed_by: LeaseHolder) -> Result<(), SnapshotError> {
    release_statement(id, claimed_by)
        .to_query()
        .execute(pool)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::snapshot::preparation_claim::{
        claim_statement,
        claimed_preparation_statement,
        next_preparing_statement,
        release_statement,
        shared_claim_statement,
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_schema::branch_schema_version::BranchSchemaVersion;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
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
    fn a_clone_is_not_claimed_while_its_source_is_preparing() {
        let current = BranchSchemaVersion::try_from(21_i64).expect("a positive version");
        let sql = next_preparing_statement(current).to_string(PostgresQueryBuilder);
        let waiting_on_source = concat!(
            r#"NOT EXISTS(SELECT 1 FROM "snapshots" AS "clone_source" JOIN "snapshot_preparation" AS "clone_source_preparation" "#,
            r#"ON "clone_source_preparation"."snapshot_key" = "clone_source"."snapshot_key" "#,
            r#"WHERE "clone_source"."id" = "snapshots"."clone_source_id" AND "clone_source_preparation"."status" = 1)"#,
        );
        assert!(sql.contains(waiting_on_source), "{sql}");
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

    #[test]
    fn a_restart_locks_only_a_preparation_this_worker_still_claims() {
        let worker = LeaseHolder::from(Uuid::from_u128(1));
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let sql = claimed_preparation_statement(&id, worker).to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"snapshots\".\"id\" = 'urn:ngsi-ld:Snapshot:s'"), "{sql}");
        assert!(
            sql.contains(&format!("\"snapshot_preparation\".\"claimed_by\" = '{}'", Uuid::from_u128(1))),
            "{sql}"
        );
        assert!(sql.contains("\"snapshot_preparation\".\"status\" = 1"), "{sql}");
        assert!(sql.ends_with("FOR UPDATE"), "{sql}");
    }

    #[test]
    fn a_clone_copy_key_share_locks_only_a_preparation_this_worker_still_claims() {
        let worker = LeaseHolder::from(Uuid::from_u128(1));
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let sql = shared_claim_statement(&id, worker).to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"snapshots\".\"id\" = 'urn:ngsi-ld:Snapshot:s'"), "{sql}");
        assert!(
            sql.contains(&format!("\"snapshot_preparation\".\"claimed_by\" = '{}'", Uuid::from_u128(1))),
            "{sql}"
        );
        assert!(sql.contains("\"snapshot_preparation\".\"status\" = 1"), "{sql}");
        assert!(sql.ends_with("FOR KEY SHARE"), "both rows are fenced: {sql}");
        assert!(!sql.contains("FOR UPDATE"), "{sql}");
    }

    #[test]
    fn the_restart_lock_stays_for_update_while_the_copy_fence_is_key_share() {
        let worker = LeaseHolder::from(Uuid::from_u128(1));
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let locked = claimed_preparation_statement(&id, worker).to_string(PostgresQueryBuilder);
        let fenced = shared_claim_statement(&id, worker).to_string(PostgresQueryBuilder);
        let locked_rows = locked.strip_suffix(" FOR UPDATE").expect("a restart locks FOR UPDATE");
        let fenced_rows = fenced.strip_suffix(" FOR KEY SHARE").expect("a copy fences FOR KEY SHARE");
        assert_eq!(locked_rows, fenced_rows, "both select the same row");
    }

    #[test]
    fn a_release_clears_the_whole_claim_of_a_preparation_this_worker_holds() {
        let worker = LeaseHolder::from(Uuid::from_u128(1));
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let sql = release_statement(&id, worker).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with("UPDATE \"snapshot_preparation\" SET \"claimed_by\" = NULL, \"claimed_at\" = NULL, \"lease_until\" = NULL"),
            "{sql}"
        );
        assert!(
            sql.contains(&format!("\"claimed_by\" = '{}'", Uuid::from_u128(1))),
            "only this worker's claim is released: {sql}"
        );
        assert!(sql.contains("\"status\" = 1"), "only a preparing Snapshot is released: {sql}");
    }
}
