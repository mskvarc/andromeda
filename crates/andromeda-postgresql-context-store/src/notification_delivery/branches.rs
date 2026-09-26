//! Enumerating the context branches a per-branch delivery drain sweeps.
//!
//! Deliveries live in the queue of the branch whose subscription matched the
//! change: `public` for the live branch and `snap_<snapshot_key>` for each
//! published Snapshot branch. A drain therefore visits the live branch and every
//! published Snapshot branch in turn. A Snapshot still preparing has no
//! published branch yet, only its private staging branch, which holds no
//! subscription a client created, so it is not visited. A branch reaped between
//! this enumeration and the claim shows up as a missing schema, which the
//! caller skips rather than fails on. A
//! quarantined Snapshot branch -- one whose schema is behind this binary -- is
//! not visited at all: its queue relations may not have the shape the claim
//! expects, and its deliveries wait until its upgrade succeeds.

use crate::{
    error::ContextStoreError,
    migration_set::branch::{BranchSchemaVersion, current_branch_version},
    snapshot::{branch_version::branch_is_current, record::status_is_preparing},
};
use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_lexicon::uri::UriParseError;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use sea_query::{Expr, ExprTrait, JoinType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, postgres::PgPool};

/// The branches a per-branch drain sweeps: the live branch, then every
/// current Snapshot branch published under `snap_<snapshot_key>`.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the Snapshot enumeration fails,
/// [`ContextStoreError::BranchVersion`] when the embedded branch migration set
/// has no version, or [`ContextStoreError::Internal`] when a stored Snapshot key or URI cannot
/// be read back as its domain type.
pub(super) async fn drainable_targets(pool: &PgPool) -> Result<Vec<ContextTarget>, ContextStoreError> {
    let (sql, values) = drainable_snapshots_statement(current_branch_version()?).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(pool).await?;

    let mut targets = Vec::with_capacity(rows.len() + 1);
    targets.push(ContextTarget::Live);
    for row in rows {
        let raw_key: i64 = row.try_get("snapshot_key")?;
        let raw_id: String = row.try_get("id")?;
        let key = SnapshotKey::try_from(raw_key).map_err(|error| ContextStoreError::Internal(format!("snapshot branch key: {error}")))?;
        let id = raw_id
            .parse()
            .map_err(|error: UriParseError| ContextStoreError::Internal(format!("snapshot branch uri: {error}")))?;
        targets.push(ContextTarget::Snapshot(SnapshotTarget::published(key, id)));
    }
    Ok(targets)
}

/// Every published Snapshot whose branch schema is current, with its URI.
fn drainable_snapshots_statement(current: BranchSchemaVersion) -> SelectStatement {
    Query::select()
        .columns([(Snapshots::Table, Snapshots::SnapshotKey), (Snapshots::Table, Snapshots::Id)])
        .from(Snapshots::Table)
        .join(
            JoinType::Join,
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        )
        .and_where(branch_is_current(current))
        .and_where(status_is_preparing().not())
        .to_owned()
}

/// Whether a persistence failure is a Snapshot branch reaped out from under the
/// drain: its schema or a relation in it no longer exists.
///
/// PostgreSQL raises `3F000` (`invalid_schema_name`) or `42P01`
/// (`undefined_table`); either means the branch was dropped between the
/// enumeration and the claim, so the drain moves on rather than failing.
#[must_use]
pub(super) fn is_reaped_branch(error: &ContextStoreError) -> bool {
    let ContextStoreError::Database(sqlx::Error::Database(database_error)) = error else {
        return false;
    };
    matches!(database_error.code().as_deref(), Some("3F000" | "42P01"))
}

#[cfg(test)]
mod tests {
    use crate::{migration_set::branch::BranchSchemaVersion, notification_delivery::branches::drainable_snapshots_statement};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_quarantined_snapshot_branch_is_not_drained() {
        let current = BranchSchemaVersion::try_from(21_i64).expect("a positive version");
        let sql = drainable_snapshots_statement(current).to_string(PostgresQueryBuilder);
        assert!(sql.contains("JOIN \"snapshot_preparation\""), "{sql}");
        assert!(sql.contains("\"snapshot_preparation\".\"branch_schema_version\" >= 21"), "{sql}");
    }

    #[test]
    fn a_snapshot_still_preparing_has_no_published_branch_to_drain() {
        let current = BranchSchemaVersion::try_from(21_i64).expect("a positive version");
        let sql = drainable_snapshots_statement(current).to_string(PostgresQueryBuilder);
        assert!(sql.contains("NOT \"snapshot_preparation\".\"status\" = 1"), "{sql}");
    }
}
