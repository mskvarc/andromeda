//! The durable evidence of one tenant's inspection.

use crate::{
    error::MaintenanceError,
    evidence::inspection_result::InspectionResult,
    state::{RegisteredTenant, TenantKey, execute_update},
};
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTenants;
use sea_query::{Expr, ExprTrait, InsertStatement, OnConflict, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgConnection;

/// Persist that a tenant received its fair turn even when it was unavailable.
pub(crate) async fn mark_inspected(connection: &mut PgConnection, tenant: &RegisteredTenant, result: InspectionResult) -> Result<(), MaintenanceError> {
    ensure_tenant_state(connection, tenant).await?;
    let mut statement = Query::update();
    statement
        .table(PostgresqlMaintenanceTenants::Table)
        .value(PostgresqlMaintenanceTenants::LastInspectedAt, Expr::current_timestamp())
        .value(PostgresqlMaintenanceTenants::LastResult, result.code().code())
        .and_where(Expr::col(PostgresqlMaintenanceTenants::TenantKey).eq(tenant.key.0));
    if result.concludes_the_pass() {
        statement.value(PostgresqlMaintenanceTenants::ConsecutiveFailures, 0);
    }
    execute_update(connection, statement.clone()).await
}

/// Record that reaching one tenant database failed without starving later tenants.
pub(crate) async fn mark_tenant_failure(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<(), MaintenanceError> {
    ensure_tenant_state(connection, tenant).await?;
    let statement = Query::update()
        .table(PostgresqlMaintenanceTenants::Table)
        .value(PostgresqlMaintenanceTenants::LastInspectedAt, Expr::current_timestamp())
        .value(
            PostgresqlMaintenanceTenants::ConsecutiveFailures,
            Expr::col(PostgresqlMaintenanceTenants::ConsecutiveFailures).add(1),
        )
        .value(PostgresqlMaintenanceTenants::LastResult, InspectionResult::Unavailable.code().code())
        .and_where(Expr::col(PostgresqlMaintenanceTenants::TenantKey).eq(tenant.key.0))
        .to_owned();
    execute_update(connection, statement).await
}

/// Insert a tenant-state row if this is its first maintenance pass.
async fn ensure_tenant_state(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<(), MaintenanceError> {
    let (sql, values) = tenant_state_insert(tenant.key)?.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

/// Build the idempotent tenant-state insert.
fn tenant_state_insert(tenant: TenantKey) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTenants::Table)
        .columns([PostgresqlMaintenanceTenants::TenantKey])
        .values([tenant.0.into()])?
        .on_conflict(OnConflict::column(PostgresqlMaintenanceTenants::TenantKey).do_nothing().to_owned());
    Ok(statement.clone())
}
