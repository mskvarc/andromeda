//! Publishing a prepared Snapshot branch: promoting a staged capture, and
//! cloning one Snapshot's published branch into another before promoting it.
//!
//! Both paths end the same way -- the staging schema is renamed to the published
//! schema and the preparation row leaves `preparing` -- and both run in a single
//! transaction so the rename and the control-row update commit together.

use crate::snapshot::{
    branch_schema::{promote_staging_schema, publish_empty_branch},
    branch_version::{condition_of, current_version},
    clone::clone_branch,
    persistence_error,
    preparation_claim::lock_claimed_preparation,
    record::status_is_preparing,
    status_code::stored_status,
    status_outbox::enqueue_terminal_notification,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, SnapshotStatus};
use andromeda_postgresql_branch::snapshot_schema_name;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::clock::now;
use andromeda_snapshot::{branch_condition::BranchCondition, error::SnapshotError};
use sea_query::{Expr, ExprTrait, JoinType, Keyword, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Postgres, Transaction};

/// Publish one query capture -- what it staged on success, an empty branch on
/// failure, so a failed Snapshot stays addressable -- and record the terminal
/// status, all in one transaction.
pub(crate) async fn promote_staged_capture(
    pool: &PgPool,
    id: &Identifier,
    claimed_by: LeaseHolder,
    status: SnapshotStatus,
    queries_details: Option<&[ExecutionResultDetails]>,
    temporal_queries_details: Option<&[ExecutionResultDetails]>,
) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let snapshot_key = lock_preparing_key(&mut transaction, id, claimed_by).await?;
    if status == SnapshotStatus::Failure {
        publish_empty_branch(&mut transaction, snapshot_key).await.map_err(persistence_error)?;
    } else {
        promote_staging_schema(&mut transaction, snapshot_key).await.map_err(persistence_error)?;
    }
    finalize_preparation(&mut transaction, snapshot_key, claimed_by, status, queries_details, temporal_queries_details).await?;
    enqueue_terminal_notification(&mut transaction, id).await?;
    transaction.commit().await.map_err(persistence_error)
}

/// Copy a source Snapshot's published branch into the claimed target's staging
/// branch, then publish it, in one transaction.
pub(crate) async fn promote_context_clone(pool: &PgPool, source_id: &Identifier, target_id: &Identifier, claimed_by: LeaseHolder) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let source_key = source_snapshot_key(&mut transaction, source_id).await?;
    let target_key = lock_preparing_key(&mut transaction, target_id, claimed_by).await?;
    let source_schema = snapshot_schema_name(source_key, SnapshotBranch::Published);
    let target_schema = snapshot_schema_name(target_key, SnapshotBranch::Staging);
    clone_branch(&mut transaction, &source_schema, &target_schema)
        .await
        .map_err(persistence_error)?;
    promote_staging_schema(&mut transaction, target_key).await.map_err(persistence_error)?;
    finalize_preparation(&mut transaction, target_key, claimed_by, SnapshotStatus::Success, None, None).await?;
    enqueue_terminal_notification(&mut transaction, target_id).await?;
    transaction.commit().await.map_err(persistence_error)
}

/// Resolve, and row-lock, the preparing preparation row of one Snapshot claimed
/// by this replica, refusing a claim that is no longer held.
async fn lock_preparing_key(transaction: &mut Transaction<'_, Postgres>, id: &Identifier, claimed_by: LeaseHolder) -> Result<SnapshotKey, SnapshotError> {
    lock_claimed_preparation(transaction, id, claimed_by)
        .await?
        .ok_or_else(|| SnapshotError::internal(format!("Snapshot {id} is no longer preparing under this claim")))
}

/// Resolve a clone source's surrogate key by URI, refusing a quarantined source:
/// its branch schema is behind this binary's branch relations, so a
/// column-for-column copy into the target could not be trusted.
async fn source_snapshot_key(transaction: &mut Transaction<'_, Postgres>, source_id: &Identifier) -> Result<SnapshotKey, SnapshotError> {
    let current = current_version()?;
    let select = Query::select()
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .column((SnapshotPreparation::Table, SnapshotPreparation::BranchSchemaVersion))
        .from(Snapshots::Table)
        .join(
            JoinType::Join,
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        )
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(source_id.as_str()))
        .to_owned();
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let (raw, version) = sqlx::query_as_with::<_, (i64, i32), _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| SnapshotError::internal(format!("Clone source {source_id} does not exist")))?;
    match condition_of(version, current) {
        BranchCondition::Current => SnapshotKey::try_from(raw).map_err(|error| SnapshotError::internal(error.to_string())),
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
        .value(SnapshotPreparation::QueriesDetails, queries_details.map(serde_json::to_value).transpose()?)
        .value(
            SnapshotPreparation::TemporalQueriesDetails,
            temporal_queries_details.map(serde_json::to_value).transpose()?,
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
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    if result.rows_affected() == 1 {
        Ok(())
    } else {
        Err(SnapshotError::internal("Snapshot preparation row changed under the promotion"))
    }
}
