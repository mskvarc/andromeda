//! Creating a Snapshot: its control record, its preparation row and its staging
//! branch schema, in one transaction.

use crate::{
    snapshot::{
        branch_schema::create_branch_schema,
        json_value::optional_json,
        status_code::stored_status,
        unencodable_snapshot_error::UnencodableSnapshotError,
    },
    store_fault::{persistence_error, undecodable},
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_pool::sql_state::violates_unique_constraint;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_snapshot::{error::SnapshotError, store::InsertSnapshotParams};
use sea_query::{Expr, InsertStatement, Query, SimpleExpr};
use sqlx::PgPool;
use url::Url;

/// The control-record insert of `params`, returning the database-issued
/// surrogate key.
///
/// The four documents are `json` columns bound as their JSON text cast to
/// `json`, so the server keeps each as written.
fn control_insert(params: &InsertSnapshotParams<'_>) -> Result<InsertStatement, SnapshotError> {
    let values: [SimpleExpr; 11] = [
        Expr::val(params.id.as_str()),
        optional_json(params.snapshot_queries, UnencodableSnapshotError::SnapshotQueries)?,
        optional_json(params.snapshot_temporal_queries, UnencodableSnapshotError::SnapshotTemporalQueries)?,
        optional_json(Some(&params.query_context), UnencodableSnapshotError::QueryContext)?,
        Expr::val(params.snapshot_lifetime.map(ToString::to_string)),
        Expr::val(params.snapshot_priority),
        Expr::val(params.endpoint.map(Url::as_str)),
        optional_json(params.receiver_info, UnencodableSnapshotError::ReceiverInfo)?,
        Expr::val(params.clone_source_id.map(Identifier::as_str)),
        Expr::val(params.created_at),
        Expr::val(params.expires_at),
    ];
    let mut insert = Query::insert();
    insert
        .into_table(Snapshots::Table)
        .columns([
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
        ])
        .values(values)
        .map_err(|error| persistence_error::<SnapshotError>(error))?
        .returning_col(Snapshots::SnapshotKey);
    Ok(insert)
}

/// The preparation-record insert of `params` for the Snapshot `snapshot_key`;
/// the lease pair starts null. The two details documents are `json` columns
/// bound as their JSON text.
fn preparation_insert(params: &InsertSnapshotParams<'_>, snapshot_key: SnapshotKey) -> Result<InsertStatement, SnapshotError> {
    let values: [SimpleExpr; 6] = [
        Expr::val(snapshot_key.as_i64()),
        Expr::val(stored_status(params.status).code()),
        Expr::val(params.modified_at),
        Expr::val(params.last_used_at),
        optional_json(params.queries_details, UnencodableSnapshotError::QueriesDetails)?,
        optional_json(params.temporal_queries_details, UnencodableSnapshotError::TemporalQueriesDetails)?,
    ];
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
        ])
        .values(values)
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(insert)
}

/// Write one Snapshot: the control record, its preparation row, and its empty
/// staging branch schema, all in one transaction. The published branch comes
/// into being only when the preparation renames the staging schema.
pub(crate) async fn create_snapshot(pool: &PgPool, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
    let mut tx = pool.begin().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    let snapshot_key_raw: i64 = control_insert(&params)?
        .to_query_scalar()
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| snapshot_create_error(error, params.id))?;
    let snapshot_key = SnapshotKey::try_from(snapshot_key_raw).map_err(|error| SnapshotError::from(undecodable(&error)))?;
    preparation_insert(&params, snapshot_key)?
        .to_query()
        .execute(&mut *tx)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    create_branch_schema(&mut tx, snapshot_key, SnapshotBranch::Staging)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    tx.commit().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(())
}

/// The unique constraint on `snapshots.id`.
const UQ_SNAPSHOTS_ID: &str = "uq_snapshots_id";

/// Preserve the protocol-visible duplicate-id classification from PostgreSQL.
fn snapshot_create_error(error: sqlx::Error, id: &Identifier) -> SnapshotError {
    if violates_unique_constraint(&error, UQ_SNAPSHOTS_ID) {
        return SnapshotError::already_exists(id);
    }
    persistence_error::<SnapshotError>(error)
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::tenant::tenant_migrator,
        snapshot::creation::{UQ_SNAPSHOTS_ID, control_insert, preparation_insert},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;
    use andromeda_snapshot::store::InsertSnapshotParams;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// Every constraint is named by the migrations, so the duplicate-identity
    /// refusal matches the declared name rather than one PostgreSQL generated.
    #[test]
    fn the_duplicate_id_refusal_names_the_unique_constraint_the_snapshots_table_declares() {
        let declaration = format!("CONSTRAINT {UQ_SNAPSHOTS_ID} UNIQUE (id)").to_ascii_uppercase();
        assert!(
            tenant_migrator()
                .iter()
                .any(|migration| migration.sql.as_str().to_ascii_uppercase().contains(&declaration)),
            "{declaration}"
        );
    }

    fn params(id: &Identifier) -> InsertSnapshotParams<'_> {
        let instant = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("a valid instant");
        InsertSnapshotParams {
            id,
            snapshot_queries: None,
            snapshot_temporal_queries: None,
            query_context: JsonLiteral::null(),
            snapshot_lifetime: None,
            snapshot_priority: 3,
            endpoint: None,
            receiver_info: None,
            status: SnapshotStatus::Preparing,
            clone_source_id: None,
            queries_details: None,
            temporal_queries_details: None,
            created_at: instant,
            modified_at: instant,
            expires_at: instant,
            last_used_at: None,
        }
    }

    #[test]
    fn the_control_insert_binds_eleven_values_casts_its_documents_and_returns_the_surrogate_key() {
        let id: Identifier = "urn:ngsi-ld:Snapshot:a".parse().expect("a valid identifier");
        let (sql, values) = control_insert(&params(&id)).expect("the control insert builds").build(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"RETURNING "snapshot_key""#), "the surrogate key is returned: {sql}");
        assert!(!sql.contains(r#""status""#), "the status lives on the preparation row: {sql}");
        assert_eq!(
            sql.matches("::json").count(),
            1,
            "the query context is a json document; absent documents are NULL: {sql}"
        );
        assert_eq!(values.0.len(), 8, "{sql}");
    }

    #[test]
    fn the_preparation_insert_binds_the_key_status_and_times() {
        let id: Identifier = "urn:ngsi-ld:Snapshot:a".parse().expect("a valid identifier");
        let key = SnapshotKey::try_from(7).expect("a positive key");
        let (sql, values) = preparation_insert(&params(&id), key)
            .expect("the preparation insert builds")
            .build(PostgresQueryBuilder);
        assert!(
            sql.starts_with(
                r#"INSERT INTO "snapshot_preparation" ("snapshot_key", "status", "modified_at", "last_used_at", "queries_details", "temporal_queries_details")"#
            ),
            "{sql}"
        );
        assert_eq!(values.0.len(), 4, "the two absent details documents are NULL: {sql}");
    }
}
