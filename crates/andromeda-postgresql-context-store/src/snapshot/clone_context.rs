//! Publishing a prepared Snapshot branch: promoting a staged capture or clone,
//! and copying one Snapshot's published branch into a clone's staging branch.
//!
//! Every publication ends the same way -- the staging branch is analysed, the
//! staging schema is renamed to the published schema and the preparation row
//! leaves `preparing` -- in one transaction, so the first read of a published
//! branch is planned on its statistics and the rename and the control-row
//! update commit together. The analysis samples a bounded number of rows per
//! relation, so the transaction stays short however large the branch.
//!
//! A clone's copy runs before that, in a transaction of its own: it can take as
//! long as the source is large, and it must not hold for that long a lock that
//! a lease renewal, a status read or a writer on the source waits on.

use crate::{
    snapshot::{
        branch_schema::{analyze_branch_schema, promote_staging_schema, publish_empty_branch},
        branch_version::{condition_of, current_version},
        clone::clone_branch,
        json_value::optional_json,
        persistence_error,
        preparation_claim::{lock_claimed_preparation, share_claimed_preparation},
        record::status_is_preparing,
        status_code::stored_status,
        status_outbox::enqueue_terminal_notification,
    },
    store_fault::undecodable,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, SnapshotStatus};
use andromeda_postgresql_branch::snapshot_schema_name;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::{clock::now, transaction::repeatable_read};
use andromeda_snapshot::{branch_condition::BranchCondition, error::SnapshotError, snapshot_invariant::SnapshotInvariant};
use sea_query::{Expr, ExprTrait, JoinType, Keyword, LockType, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Transaction};

/// Publish one prepared branch -- what a capture or a clone staged on success,
/// its statistics gathered first, an empty branch on failure, so a failed
/// Snapshot stays addressable -- and record the terminal status, all in one
/// transaction.
pub(crate) async fn promote_staged_branch(
    pool: &PgPool,
    id: &Identifier,
    claimed_by: LeaseHolder,
    status: SnapshotStatus,
    queries_details: Option<&[ExecutionResultDetails]>,
    temporal_queries_details: Option<&[ExecutionResultDetails]>,
) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(|error| persistence_error(error))?;
    let snapshot_key = lock_preparing_key(&mut transaction, id, claimed_by).await?;
    if status == SnapshotStatus::Failure {
        publish_empty_branch(&mut transaction, snapshot_key)
            .await
            .map_err(|error| persistence_error(error))?;
    } else {
        analyze_branch_schema(&mut transaction, snapshot_key, SnapshotBranch::Staging)
            .await
            .map_err(|error| persistence_error(error))?;
        promote_staging_schema(&mut transaction, snapshot_key)
            .await
            .map_err(|error| persistence_error(error))?;
    }
    finalize_preparation(&mut transaction, snapshot_key, claimed_by, status, queries_details, temporal_queries_details).await?;
    enqueue_terminal_notification(&mut transaction, id).await?;
    transaction.commit().await.map_err(|error| persistence_error(error))
}

/// Copy a source Snapshot's published branch into the claimed target's staging
/// branch, then publish it.
///
/// The copy and the publication are two transactions. The copy runs at
/// REPEATABLE READ, and the lease renewal rewrites the target's preparation
/// row throughout it; a REPEATABLE READ transaction updating a row changed
/// after its snapshot fails with a serialization error, so the publication's
/// update of that row cannot share the copy's transaction.
pub(crate) async fn promote_context_clone(pool: &PgPool, source_id: &Identifier, target_id: &Identifier, claimed_by: LeaseHolder) -> Result<(), SnapshotError> {
    copy_clone_source(pool, source_id, target_id, claimed_by).await?;
    promote_staged_branch(pool, target_id, claimed_by, SnapshotStatus::Success, None, None).await
}

/// Copy a source Snapshot's published branch into the claimed target's staging
/// branch, as of one moment, and commit the copy unpublished.
///
/// Every copy statement reads the snapshot the transaction's first statement
/// took, so the copied relations agree with each other -- a copied census
/// counts exactly the copied search rows -- without locking the source's
/// relations against writers or the census fold. Clause 5.16.2.4 leaves the
/// moment a concurrently written source is copied at unspecified; it is the
/// start of the copy. The two control records are held only `FOR KEY SHARE`,
/// which keeps the target's claim and both Snapshots in place without blocking
/// renewals, touches or metadata changes. A failure after the commit leaves
/// only staged rows, which the next restart or claim of the target discards.
async fn copy_clone_source(pool: &PgPool, source_id: &Identifier, target_id: &Identifier, claimed_by: LeaseHolder) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(|error| persistence_error(error))?;
    sqlx::query(repeatable_read())
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    let target_key = share_claimed_preparation(&mut transaction, target_id, claimed_by)
        .await?
        .ok_or_else(|| claim_lost(target_id))?;
    let source_key = source_snapshot_key(&mut transaction, source_id).await?;
    let source_schema = snapshot_schema_name(source_key, SnapshotBranch::Published);
    let target_schema = snapshot_schema_name(target_key, SnapshotBranch::Staging);
    clone_branch(&mut transaction, &source_schema, &target_schema)
        .await
        .map_err(|error| persistence_error(error))?;
    transaction.commit().await.map_err(|error| persistence_error(error))
}

/// The refusal of a preparation whose claim this replica no longer holds.
fn claim_lost(id: &Identifier) -> SnapshotError {
    SnapshotError::from(SnapshotInvariant::ClaimLost {
        snapshot: id.clone(), // clone: the error owns the Snapshot it names
    })
}

/// Resolve, and row-lock, the preparing preparation row of one Snapshot claimed
/// by this replica, refusing a claim that is no longer held.
async fn lock_preparing_key(transaction: &mut Transaction<'_, Postgres>, id: &Identifier, claimed_by: LeaseHolder) -> Result<SnapshotKey, SnapshotError> {
    lock_claimed_preparation(transaction, id, claimed_by).await?.ok_or_else(|| claim_lost(id))
}

/// Select a clone source's surrogate key and branch version by URI, holding its
/// control record `FOR KEY SHARE`.
///
/// Every path that deletes a Snapshot takes `FOR UPDATE` on its control record
/// before it drops the branch schema, so a delete, expiry or eviction of the
/// source waits for the copy, or skips the source, instead of dropping the
/// relations being copied. Only the control record is fenced: the preparation
/// row takes the source's last-used touches, which must not conflict with it.
fn source_key_statement(source_id: &Identifier) -> SelectStatement {
    Query::select()
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .column((SnapshotPreparation::Table, SnapshotPreparation::BranchSchemaVersion))
        .from(Snapshots::Table)
        .join(
            JoinType::Join,
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        )
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(source_id.as_str()))
        .lock_with_tables(LockType::KeyShare, [Snapshots::Table])
        .to_owned()
}

/// Resolve, and fence, a clone source's surrogate key by URI, refusing a
/// quarantined source: its branch schema is behind this binary's branch
/// relations, so a column-for-column copy into the target could not be trusted.
async fn source_snapshot_key(transaction: &mut Transaction<'_, Postgres>, source_id: &Identifier) -> Result<SnapshotKey, SnapshotError> {
    let current = current_version()?;
    let (sql, values) = source_key_statement(source_id).build_sqlx(PostgresQueryBuilder);
    let (raw, version) = sqlx::query_as_with::<_, (i64, i32), _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| persistence_error(error))?
        .ok_or_else(|| {
            SnapshotError::from(SnapshotInvariant::CloneSourceMissing {
                source_snapshot: source_id.clone(), // clone: the error owns the Snapshot it names
            })
        })?;
    match condition_of(version, current) {
        BranchCondition::Current => SnapshotKey::try_from(raw).map_err(|error| SnapshotError::from(undecodable(&error))),
        BranchCondition::Quarantined => Err(SnapshotError::quarantined(source_id)),
    }
}

/// Record the terminal preparation state, clearing the lease; must match the
/// still-preparing row it locked.
async fn finalize_preparation(
    transaction: &mut Transaction<'_, Postgres>,
    snapshot_key: SnapshotKey,
    claimed_by: LeaseHolder,
    status: SnapshotStatus,
    queries_details: Option<&[ExecutionResultDetails]>,
    temporal_queries_details: Option<&[ExecutionResultDetails]>,
) -> Result<(), SnapshotError> {
    let statement = Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::Status, stored_status(status).code())
        .value(SnapshotPreparation::QueriesDetails, optional_json(queries_details)?)
        .value(SnapshotPreparation::TemporalQueriesDetails, optional_json(temporal_queries_details)?)
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
        .execute(&mut **transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    if result.rows_affected() == 1 {
        Ok(())
    } else {
        Err(SnapshotInvariant::PreparationChangedUnderPromotion.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot::clone_context::source_key_statement;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_clone_source_is_read_under_a_key_share_lock_of_its_control_record() {
        let source = "urn:ngsi-ld:Snapshot:src".parse().expect("valid snapshot uri");
        let sql = source_key_statement(&source).to_string(PostgresQueryBuilder);
        assert!(sql.contains("JOIN \"snapshot_preparation\""), "{sql}");
        assert!(sql.contains("\"snapshots\".\"id\" = 'urn:ngsi-ld:Snapshot:src'"), "{sql}");
        assert!(sql.contains("\"snapshot_preparation\".\"branch_schema_version\""), "{sql}");
        assert!(sql.ends_with("FOR KEY SHARE OF \"snapshots\""), "{sql}");
    }
}
