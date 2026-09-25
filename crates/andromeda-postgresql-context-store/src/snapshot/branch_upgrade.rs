//! Bringing every Snapshot's branch schemas up to the binary's branch version.
//!
//! A branch migration appended by a later release upgrades the live branch as
//! part of the tenant migration set, and every Snapshot branch here: the same
//! migrator that created each schema is run inside it again, over one direct
//! connection whose search path points at the schema being upgraded. Each
//! Snapshot is upgraded on its own. A success records the new version; a
//! failure records why and leaves the version behind, which quarantines that
//! Snapshot alone -- the tenant and every other Snapshot keep serving -- and
//! the next run retries it.

use crate::{
    error::ContextStoreError,
    migration_set::branch::{BranchMigrationSet, BranchSchemaVersion, current_branch_version},
    snapshot::branch_schema::{branch_search_path, steer_search_path},
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::snapshot_schema_name;
use andromeda_postgresql_schema::snapshot::SnapshotPreparation;
use andromeda_postgresql_seaquery::{introspection::schema_exists, setting::SettingScope};
use sea_query::{Expr, ExprTrait, Keyword, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Connection, PgConnection, PgPool};

/// The longest upgrade failure recorded; the column's check pins the same bound.
const UPGRADE_ERROR_MAX_CHARS: usize = 2048;

/// Whether any Snapshot's branch schemas are behind the binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotBranchState {
    /// Every Snapshot carries the current branch version.
    Current,
    /// At least one Snapshot is behind, or quarantined by an earlier failure.
    Behind,
}

/// What one upgrade run did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BranchUpgradeReport {
    /// Snapshots brought to the current branch version.
    pub upgraded: usize,
    /// Snapshots whose upgrade failed and which are now quarantined.
    pub quarantined: usize,
}

/// Probe whether any Snapshot's branch schemas are behind the binary.
///
/// One `EXISTS` statement, safe over a transaction-pooling proxy, so a warm
/// tenant load pays for nothing more when every Snapshot is current.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the embedded branch set has no version or
/// the probe fails.
pub async fn snapshot_branch_state(pool: &PgPool) -> Result<SnapshotBranchState, ContextStoreError> {
    let current = current_branch_version()?;
    let (sql, values) = Query::select().expr(Expr::exists(behind_statement(current))).build_sqlx(PostgresQueryBuilder);
    let behind: bool = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?;
    Ok(if behind { SnapshotBranchState::Behind } else { SnapshotBranchState::Current })
}

/// Upgrade every Snapshot whose branch schemas are behind `set`.
///
/// The broker passes [`BranchMigrationSet::embedded`]; a rehearsal passes the
/// set a later release would carry.
///
/// `pool` must reach PostgreSQL directly: each branch migration takes the
/// migrator's session advisory lock, and the search path is set for the whole
/// session of the connection the migrations run on. That connection is
/// detached from the pool and closed afterwards, so the session setting never
/// leaks into a connection the request path later borrows.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the Snapshots behind cannot be listed or
/// an outcome cannot be recorded. A failing branch migration is not an error
/// of the run: it quarantines its Snapshot and the run moves on.
pub async fn upgrade_snapshot_branches(pool: &PgPool, set: &BranchMigrationSet) -> Result<BranchUpgradeReport, ContextStoreError> {
    let current = set.version()?;
    let (sql, values) = behind_statement(current).build_sqlx(PostgresQueryBuilder);
    let behind = sqlx::query_scalar_with::<_, i64, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await?;
    if behind.is_empty() {
        return Ok(BranchUpgradeReport::default());
    }
    let mut connection = pool.acquire().await?.detach();
    let mut report = BranchUpgradeReport::default();
    for raw in behind {
        let key = SnapshotKey::try_from(raw).map_err(|error| ContextStoreError::Internal(format!("snapshot branch key: {error}")))?;
        match upgrade_snapshot(&mut connection, set, key).await {
            Ok(()) => {
                record(pool, upgraded_statement(key, current)).await?;
                report.upgraded += 1;
            }
            Err(error) => {
                tracing::warn!(snapshot_key = key.as_i64(), %error, event = "snapshot_branch_quarantined", "a Snapshot branch upgrade failed; the Snapshot is quarantined");
                record(pool, quarantined_statement(key, &error.to_string())).await?;
                report.quarantined += 1;
            }
        }
    }
    connection.close().await?;
    Ok(report)
}

/// Run the branch migrator in every branch schema one Snapshot still has; the
/// staging schema is gone once a Snapshot is published.
async fn upgrade_snapshot(connection: &mut PgConnection, set: &BranchMigrationSet, key: SnapshotKey) -> Result<(), ContextStoreError> {
    for branch in [SnapshotBranch::Published, SnapshotBranch::Staging] {
        let schema = snapshot_schema_name(key, branch);
        let (sql, values) = Query::select().expr(schema_exists(&schema)).build_sqlx(PostgresQueryBuilder);
        let present: bool = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_one(&mut *connection)
            .await?;
        if present {
            steer_search_path(connection, &branch_search_path(&schema), SettingScope::Session).await?;
            // `run_direct` rather than `run`: see `create_branch_schema`.
            set.migrator(&schema).run_direct(None, &mut *connection, false).await?;
        }
    }
    Ok(())
}

/// The surrogate keys of every Snapshot behind `current`.
fn behind_statement(current: BranchSchemaVersion) -> SelectStatement {
    Query::select()
        .column(SnapshotPreparation::SnapshotKey)
        .from(SnapshotPreparation::Table)
        .and_where(Expr::col(SnapshotPreparation::BranchSchemaVersion).lt(current.get()))
        .to_owned()
}

/// Record a successful upgrade, clearing any earlier failure.
fn upgraded_statement(key: SnapshotKey, current: BranchSchemaVersion) -> UpdateStatement {
    Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::BranchSchemaVersion, current.get())
        .value(SnapshotPreparation::BranchUpgradeError, SimpleExpr::Keyword(Keyword::Null))
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(key.as_i64()))
        .to_owned()
}

/// Record a failed upgrade, leaving the version behind.
fn quarantined_statement(key: SnapshotKey, error: &str) -> UpdateStatement {
    let bounded: String = error.chars().take(UPGRADE_ERROR_MAX_CHARS).collect();
    Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::BranchUpgradeError, bounded)
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(key.as_i64()))
        .to_owned()
}

/// Execute one outcome update.
async fn record(pool: &PgPool, statement: UpdateStatement) -> Result<(), ContextStoreError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::branch::BranchSchemaVersion,
        snapshot::branch_upgrade::{UPGRADE_ERROR_MAX_CHARS, behind_statement, quarantined_statement, upgraded_statement},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn key() -> SnapshotKey {
        SnapshotKey::try_from(7).expect("a positive key")
    }

    fn version() -> BranchSchemaVersion {
        BranchSchemaVersion::try_from(21_i64).expect("a positive version")
    }

    #[test]
    fn the_snapshots_behind_are_those_below_the_current_branch_version() {
        let sql = behind_statement(version()).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            "SELECT \"snapshot_key\" FROM \"snapshot_preparation\" WHERE \"branch_schema_version\" < 21"
        );
    }

    #[test]
    fn a_successful_upgrade_records_the_version_and_clears_the_error() {
        let sql = upgraded_statement(key(), version()).to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"branch_schema_version\" = 21"), "{sql}");
        assert!(sql.contains("\"branch_upgrade_error\" = NULL"), "{sql}");
        assert!(sql.contains("WHERE \"snapshot_key\" = 7"), "{sql}");
    }

    #[test]
    fn a_failed_upgrade_records_a_bounded_error_and_leaves_the_version_behind() {
        let long = "x".repeat(UPGRADE_ERROR_MAX_CHARS + 10);
        let (sql, values) = quarantined_statement(key(), &long).build(PostgresQueryBuilder);
        assert!(!sql.contains("branch_schema_version"), "{sql}");
        let bound = format!("{:?}", values.0[0]);
        assert!(
            bound.contains(&"x".repeat(UPGRADE_ERROR_MAX_CHARS)),
            "the bound error keeps the first 2048 characters"
        );
        assert!(!bound.contains(&"x".repeat(UPGRADE_ERROR_MAX_CHARS + 1)), "and no more");
    }
}
