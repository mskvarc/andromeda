//! Creating a Snapshot: its control record, its preparation row and its staging
//! branch schema, in one transaction.

use crate::{
    error::violates_unique_constraint,
    snapshot::{branch_schema::create_branch_schema, branch_version::current_version, persistence_error, status_code::stored_status},
    store_fault::undecodable,
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::placeholder::external_placeholder;
use andromeda_snapshot::{error::SnapshotError, store::InsertSnapshotParams};
use sea_query::{PostgresQueryBuilder, Query};
use sqlx::PgPool;
use url::Url;

/// The immutable control columns a create binds, in placeholder order; the
/// surrogate key is issued by the database.
const fn control_columns() -> [Snapshots; 11] {
    [
        Snapshots::Id,
        Snapshots::SnapshotQueries,
        Snapshots::SnapshotTemporalQueries,
        Snapshots::QueryContext,
        Snapshots::SnapshotLifetime,
        Snapshots::SnapshotPriority,
        Snapshots::Endpoint,
        Snapshots::ReceiverInfo,
        Snapshots::CloneSourceId,
        Snapshots::CreatedAt,
        Snapshots::ExpiresAt,
    ]
}

/// The control-record insert, its eleven values bound by the caller, returning
/// the database-issued surrogate key.
fn control_insert_sql() -> Result<String, SnapshotError> {
    let mut insert = Query::insert();
    insert
        .into_table(Snapshots::Table)
        .columns(control_columns())
        .values((1..=11).map(external_placeholder))
        .map_err(persistence_error)?
        .returning_col(Snapshots::SnapshotKey);
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The preparation-record insert; the lease triple starts null, and the branch
/// version is the binary's own, since the branch schemas are created by it.
fn preparation_insert_sql() -> Result<String, SnapshotError> {
    let mut insert = Query::insert();
    insert
        .into_table(SnapshotPreparation::Table)
        .columns([
            SnapshotPreparation::SnapshotKey,
            SnapshotPreparation::Status,
            SnapshotPreparation::ModifiedAt,
            SnapshotPreparation::LastUsedAt,
            SnapshotPreparation::QueriesDetails,
            SnapshotPreparation::TemporalQueriesDetails,
            SnapshotPreparation::BranchSchemaVersion,
        ])
        .values((1..=7).map(external_placeholder))
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Write one Snapshot: the control record, its preparation row, and its empty
/// staging branch schema, all in one transaction. The published branch comes
/// into being only when the preparation renames the staging schema.
pub(crate) async fn create_snapshot(pool: &PgPool, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
    let branch_version = current_version()?;
    let mut tx = pool.begin().await.map_err(persistence_error)?;
    let control_sql = control_insert_sql()?;
    let snapshot_key_raw: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(control_sql.as_str()))
        .bind(params.id.as_str())
        .bind(params.snapshot_queries.map(serde_json::to_value).transpose()?)
        .bind(params.snapshot_temporal_queries.map(serde_json::to_value).transpose()?)
        .bind(params.query_context_json)
        .bind(params.snapshot_lifetime.map(ToString::to_string))
        .bind(params.snapshot_priority)
        .bind(params.endpoint.map(Url::as_str))
        .bind(params.receiver_info_json)
        .bind(params.clone_source_id.map(Identifier::as_str))
        .bind(params.created_at)
        .bind(params.expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| snapshot_create_error(error, params.id))?;
    let snapshot_key = SnapshotKey::try_from(snapshot_key_raw).map_err(|error| SnapshotError::from(undecodable(&error)))?;
    let preparation_sql = preparation_insert_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(preparation_sql.as_str()))
        .bind(snapshot_key.as_i64())
        .bind(stored_status(params.status).code())
        .bind(params.modified_at)
        .bind(params.last_used_at)
        .bind(params.queries_details_json)
        .bind(params.temporal_queries_details_json)
        .bind(branch_version.get())
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
    create_branch_schema(&mut tx, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(persistence_error)?;
    tx.commit().await.map_err(persistence_error)?;
    Ok(())
}

/// The unique constraint migration 0014 names on `snapshots.id`.
const UQ_SNAPSHOTS_ID: &str = "uq_snapshots_id";

/// Preserve the protocol-visible duplicate-id classification from PostgreSQL.
fn snapshot_create_error(error: sqlx::Error, id: &Identifier) -> SnapshotError {
    if violates_unique_constraint(&error, UQ_SNAPSHOTS_ID) {
        return SnapshotError::already_exists(id);
    }
    persistence_error(error)
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::tenant::tenant_migrator,
        snapshot::creation::{UQ_SNAPSHOTS_ID, control_insert_sql, preparation_insert_sql},
    };

    /// Migration 0014 names every constraint, so the duplicate-identity
    /// refusal matches the declared name rather than one PostgreSQL generated.
    #[test]
    fn the_duplicate_id_refusal_names_the_constraint_migration_0014_declares() {
        let declaration = format!("CONSTRAINT {UQ_SNAPSHOTS_ID} UNIQUE (id)");
        assert!(
            tenant_migrator().iter().any(|migration| migration.sql.as_str().contains(&declaration)),
            "{declaration}"
        );
    }

    #[test]
    fn the_control_insert_binds_eleven_values_and_returns_the_surrogate_key() {
        let sql = control_insert_sql().expect("the control insert builds");
        for marker in 1..=11 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$12"), "the control insert binds exactly eleven values: {sql}");
        assert!(sql.contains("RETURNING \"snapshot_key\""), "the surrogate key is returned: {sql}");
        assert!(!sql.contains("\"status\""), "the status lives on the preparation row: {sql}");
    }

    #[test]
    fn the_preparation_insert_binds_the_status_code_and_the_branch_version() {
        let sql = preparation_insert_sql().expect("the preparation insert builds");
        assert!(sql.contains("snapshot_preparation"), "{sql}");
        assert!(sql.contains("\"branch_schema_version\""), "{sql}");
        assert!(sql.contains("$7"), "{sql}");
        assert!(!sql.contains("::"), "no enum cast remains: {sql}");
    }
}
