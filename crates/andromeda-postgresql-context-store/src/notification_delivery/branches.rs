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
    migration_set::branch::current_branch_version,
    snapshot::{record::status_is_preparing, stored_snapshot_error::StoredSnapshotError},
};
use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_postgresql_pool::sql_state::SqlState;
use andromeda_postgresql_schema::{
    branch_schema_version::BranchSchemaVersion,
    snapshot::{SnapshotPreparation, Snapshots, branch_is_current},
};
use andromeda_postgresql_seaquery::statement::Statement;
use sea_query::{Expr, ExprTrait, JoinType, Query, SelectStatement};
use sqlx::{Row, postgres::PgPool};

/// The branches a per-branch drain sweeps: the live branch, then every
/// current Snapshot branch published under `snap_<snapshot_key>`.
///
/// The pending-work probe reads the same branches, so a dormant tenant is woken
/// for exactly the deliveries its drain would claim.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the Snapshot enumeration fails,
/// [`ContextStoreError::BranchVersion`] when the embedded branch migration set
/// has no version, or [`ContextStoreError::StoredSnapshot`] when a stored Snapshot key or URI cannot
/// be read back as its domain type.
pub(crate) async fn drainable_targets(pool: &PgPool) -> Result<Vec<ContextTarget>, ContextStoreError> {
    let rows = drainable_snapshots_statement(current_branch_version()?).to_query().fetch_all(pool).await?;

    let mut targets = Vec::with_capacity(rows.len() + 1);
    targets.push(ContextTarget::Live);
    for row in rows {
        let raw_key: i64 = row.try_get("snapshot_key")?;
        let raw_id: String = row.try_get("id")?;
        let key = SnapshotKey::try_from(raw_key).map_err(StoredSnapshotError::Key)?;
        let id = raw_id.parse().map_err(StoredSnapshotError::Id)?;
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
pub(crate) fn is_reaped_branch(error: &ContextStoreError) -> bool {
    let ContextStoreError::Database(database_error) = error else {
        return false;
    };
    is_reaped_relation(database_error)
}

/// Whether a statement failed because a schema or relation it names no longer
/// exists, as it does once the Snapshot branch it read was reaped.
#[must_use]
pub(crate) fn is_reaped_relation(error: &sqlx::Error) -> bool {
    matches!(SqlState::of(error), Some(SqlState::InvalidSchemaName | SqlState::UndefinedTable))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::notification_delivery::branches::drainable_snapshots_statement;
    use andromeda_postgresql_schema::branch_schema_version::BranchSchemaVersion;
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
