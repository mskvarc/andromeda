//! The context branch dimension of the platform target rows.
//!
//! A target row's `snapshot_key` is a member of its primary key, so it is
//! never NULL: 0 for the live branch, which no Snapshot key is, and the
//! Snapshot's key for a row of a Snapshot branch. Every writer and reader of
//! the column converts through here. A Snapshot key lives in the tenant
//! database, so nothing deletes a branch's rows when its Snapshot is reaped:
//! each visit deletes the rows of every branch whose schema it no longer finds.

use crate::{branch::Branch, error::MaintenanceError, state::RegisteredTenant};
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use andromeda_postgresql_seaquery::statement::Statement;
use sea_query::{DeleteStatement, Expr, ExprTrait, Query, SimpleExpr};
use sqlx::PgConnection;
use tracing::info;

/// The stored key of the live branch.
const LIVE_BRANCH_KEY: i64 = 0;

/// The condition selecting the target rows of `branch`.
pub(crate) fn branch_rows(branch: Branch) -> SimpleExpr {
    Expr::col(PostgresqlMaintenanceTargets::SnapshotKey).eq(stored_snapshot_key(branch))
}

/// The stored key of `branch`: 0 for the live branch, the Snapshot's key
/// otherwise.
pub(crate) fn stored_snapshot_key(branch: Branch) -> i64 {
    branch.snapshot_key().map_or(LIVE_BRANCH_KEY, SnapshotKey::as_i64)
}

/// The branch a stored key names.
///
/// # Errors
///
/// Returns [`MaintenanceError::InvalidSnapshotKey`] when the key is neither 0
/// nor a Snapshot key.
pub(crate) fn branch_of_stored_key(stored: i64) -> Result<Branch, MaintenanceError> {
    match stored {
        LIVE_BRANCH_KEY => Ok(Branch::Live),
        key => Ok(Branch::Snapshot(SnapshotKey::try_from(key)?)),
    }
}

/// Delete the tenant's target rows of every Snapshot branch not in
/// `published`, the branches whose schema the tenant database holds.
pub(crate) async fn prune_vanished_branches(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    published: &[SnapshotKey],
) -> Result<(), MaintenanceError> {
    let deleted = vanished_branch_rows_delete(tenant, published)
        .to_query()
        .execute(connection)
        .await?
        .rows_affected();
    if deleted > 0 {
        info!(tenant = tenant.log_name(), database = %tenant.database, deleted_rows = deleted, "PostgreSQL maintenance state of reaped Snapshot branches deleted");
    }
    Ok(())
}

/// Build the delete of the tenant's rows of vanished Snapshot branches.
fn vanished_branch_rows_delete(tenant: &RegisteredTenant, published: &[SnapshotKey]) -> DeleteStatement {
    let mut statement = Query::delete();
    statement
        .from_table(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::SnapshotKey).ne(LIVE_BRANCH_KEY));
    if !published.is_empty() {
        statement.and_where(Expr::col(PostgresqlMaintenanceTargets::SnapshotKey).is_not_in(published.iter().map(|key| key.as_i64())));
    }
    statement
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::Branch,
        error::MaintenanceError,
        state::{
            RegisteredTenant,
            TenantKey,
            branch_rows::{branch_of_stored_key, branch_rows, stored_snapshot_key, vanished_branch_rows_delete},
        },
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use pretty_assertions::assert_eq;
    use sea_query::{Asterisk, PostgresQueryBuilder, Query};

    fn tenant() -> RegisteredTenant {
        RegisteredTenant {
            key: TenantKey(7),
            name: None,
            database: TenantDatabaseName::from("andromeda".to_owned()),
        }
    }

    fn key(raw: i64) -> SnapshotKey {
        SnapshotKey::try_from(raw).expect("positive snapshot key")
    }

    fn condition(branch: Branch) -> String {
        Query::select()
            .column(Asterisk)
            .and_where(branch_rows(branch))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn live_rows_hold_key_zero_and_branch_rows_hold_their_snapshot_key() {
        assert_eq!(condition(Branch::Live), r#"SELECT * WHERE "snapshot_key" = 0"#);
        assert_eq!(condition(Branch::Snapshot(key(42))), r#"SELECT * WHERE "snapshot_key" = 42"#);
    }

    #[test]
    fn the_live_branch_is_stored_as_zero_and_reads_back_as_live() {
        assert_eq!(stored_snapshot_key(Branch::Live), 0);
        assert_eq!(branch_of_stored_key(0).expect("the live key"), Branch::Live);
    }

    #[test]
    fn a_snapshot_branch_is_stored_as_its_key_and_reads_back_as_it() {
        assert_eq!(stored_snapshot_key(Branch::Snapshot(key(42))), 42);
        assert_eq!(branch_of_stored_key(42).expect("a Snapshot key"), Branch::Snapshot(key(42)));
    }

    #[test]
    fn a_stored_negative_key_is_refused() {
        assert!(matches!(branch_of_stored_key(-1), Err(MaintenanceError::InvalidSnapshotKey(_))));
    }

    #[test]
    fn rows_of_branches_without_a_published_schema_are_deleted() {
        let sql = vanished_branch_rows_delete(&tenant(), &[key(3), key(9)]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"DELETE FROM "postgresql_maintenance_targets" WHERE "tenant_key" = 7 AND "snapshot_key" <> 0 AND "snapshot_key" NOT IN (3, 9)"#
        );
    }

    #[test]
    fn with_no_published_branch_every_branch_row_is_deleted_and_live_rows_stay() {
        let sql = vanished_branch_rows_delete(&tenant(), &[]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"DELETE FROM "postgresql_maintenance_targets" WHERE "tenant_key" = 7 AND "snapshot_key" <> 0"#
        );
    }
}
