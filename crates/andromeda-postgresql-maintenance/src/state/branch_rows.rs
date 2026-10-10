//! The context branch dimension of the platform target rows.
//!
//! A target row of the live branch holds no Snapshot key; a row of a Snapshot
//! branch holds that Snapshot's key. A Snapshot key lives in the tenant
//! database, so nothing deletes a branch's rows when its Snapshot is reaped:
//! each visit deletes the rows of every branch whose schema it no longer finds.

use crate::{branch::Branch, error::MaintenanceError, state::RegisteredTenant};
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use sea_query::{DeleteStatement, Expr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgConnection;
use tracing::info;

/// The condition selecting the target rows of `branch`.
pub(crate) fn branch_rows(branch: Branch) -> SimpleExpr {
    let column = Expr::col(PostgresqlMaintenanceTargets::SnapshotKey);
    match branch.snapshot_key() {
        None => column.is_null(),
        Some(key) => column.eq(key.as_i64()),
    }
}

/// The stored Snapshot key of `branch`, NULL for the live branch.
pub(crate) fn stored_snapshot_key(branch: Branch) -> Option<i64> {
    branch.snapshot_key().map(SnapshotKey::as_i64)
}

/// Delete the tenant's target rows of every Snapshot branch not in
/// `published`, the branches whose schema the tenant database holds.
pub(crate) async fn prune_vanished_branches(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    published: &[SnapshotKey],
) -> Result<(), MaintenanceError> {
    let (sql, values) = vanished_branch_rows_delete(tenant, published).build_sqlx(PostgresQueryBuilder);
    let deleted = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
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
        .and_where(Expr::col(PostgresqlMaintenanceTargets::SnapshotKey).is_not_null());
    if !published.is_empty() {
        statement.and_where(Expr::col(PostgresqlMaintenanceTargets::SnapshotKey).is_not_in(published.iter().map(|key| key.as_i64())));
    }
    statement
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        state::{
            RegisteredTenant,
            TenantKey,
            branch_rows::{branch_rows, vanished_branch_rows_delete},
        },
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_tenancy::database_name::TenantDatabaseName;
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
    fn live_rows_hold_no_snapshot_key_and_branch_rows_hold_theirs() {
        assert_eq!(condition(Branch::Live), r#"SELECT * WHERE "snapshot_key" IS NULL"#);
        assert_eq!(condition(Branch::Snapshot(key(42))), r#"SELECT * WHERE "snapshot_key" = 42"#);
    }

    #[test]
    fn rows_of_branches_without_a_published_schema_are_deleted() {
        let sql = vanished_branch_rows_delete(&tenant(), &[key(3), key(9)]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"DELETE FROM "postgresql_maintenance_targets" WHERE "tenant_key" = 7 AND "snapshot_key" IS NOT NULL AND "snapshot_key" NOT IN (3, 9)"#
        );
    }

    #[test]
    fn with_no_published_branch_every_branch_row_is_deleted_and_live_rows_stay() {
        let sql = vanished_branch_rows_delete(&tenant(), &[]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"DELETE FROM "postgresql_maintenance_targets" WHERE "tenant_key" = 7 AND "snapshot_key" IS NOT NULL"#
        );
    }
}
