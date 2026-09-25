//! Platform-database coordination, fair selection and durable evidence.
//!
//! The controller's state lives in the platform database: this module owns the
//! fleet-wide lock and the fair choice of the next tenant, [`tenant_inspection`]
//! owns what is recorded per tenant and [`target_record`] what is recorded per
//! target.

/// The durable decision and action state of one maintenance target.
pub(crate) mod target_record;
/// The durable evidence of one tenant's inspection.
pub(crate) mod tenant_inspection;

use crate::error::MaintenanceError;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_schema::{maintenance::PostgresqlMaintenanceTenants, tenancy::Tenants};
use andromeda_postgresql_seaquery::advisory_lock::{advisory_unlock, try_advisory_lock};
use andromeda_tenancy::database_name::TenantDatabaseName;
use sea_query::{Expr, ExprTrait, Iden, JoinType, NullOrdering, Order, PostgresQueryBuilder, Query, SelectStatement, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};

/// Stable key for the process-wide lock.
const GLOBAL_LOCK_KEY: &str = "andromeda-postgresql-maintenance";

/// Stable primary key of one platform tenant row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TenantId(i64);

/// One registered database selected independently of the runtime cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RegisteredTenant {
    /// Platform row identity used by durable state.
    pub(crate) id: TenantId,
    /// Absent only for the unnamed default tenant.
    pub(crate) name: Option<TenantName>,
    /// PostgreSQL database name.
    pub(crate) database: TenantDatabaseName,
}

impl RegisteredTenant {
    /// Bounded log label that never becomes a metric dimension.
    pub(crate) fn log_name(&self) -> &str {
        self.name.as_ref().map_or("<default>", TenantName::as_str)
    }
}

/// Try to obtain the one fleet-wide session advisory lock without waiting.
pub(crate) async fn try_global_lock(connection: &mut PgConnection) -> Result<bool, MaintenanceError> {
    let statement = Query::select().expr(try_advisory_lock(Expr::val(GLOBAL_LOCK_KEY))).to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}

/// Release the fleet-wide session advisory lock.
pub(crate) async fn release_global_lock(connection: &mut PgConnection) -> Result<(), MaintenanceError> {
    let statement = Query::select().expr(advisory_unlock(Expr::val(GLOBAL_LOCK_KEY))).to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let released = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?;
    if released {
        Ok(())
    } else {
        Err(MaintenanceError::Database(sqlx::Error::Protocol(
            "the PostgreSQL maintenance advisory lock was not held".to_owned(),
        )))
    }
}

/// Select the registered database with the oldest persisted inspection time.
pub(crate) async fn select_least_recently_inspected(connection: &mut PgConnection) -> Result<Option<RegisteredTenant>, MaintenanceError> {
    let (sql, values) = least_recently_inspected_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(connection).await?;
    row.map(|row| {
        let raw_name = row.try_get::<Option<String>, _>(Tenants::Name.to_string().as_str())?;
        Ok(RegisteredTenant {
            id: TenantId(row.try_get(Tenants::Id.to_string().as_str())?),
            name: raw_name.map(TenantName::try_from).transpose()?,
            database: TenantDatabaseName::from(row.try_get::<String, _>(Tenants::DbName.to_string().as_str())?),
        })
    })
    .transpose()
}

/// Fair-selection query over every active registry row, including the default.
fn least_recently_inspected_statement() -> SelectStatement {
    Query::select()
        .columns([
            (Tenants::Table, Tenants::Id),
            (Tenants::Table, Tenants::Name),
            (Tenants::Table, Tenants::DbName),
        ])
        .from(Tenants::Table)
        .join(
            JoinType::LeftJoin,
            PostgresqlMaintenanceTenants::Table,
            Expr::col((PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::TenantId)).equals((Tenants::Table, Tenants::Id)),
        )
        .and_where(Expr::col((Tenants::Table, Tenants::Status)).eq(TenantStatusCode::Active.code()))
        .order_by_with_nulls(
            (PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::LastInspectedAt),
            Order::Asc,
            NullOrdering::First,
        )
        .order_by((Tenants::Table, Tenants::Id), Order::Asc)
        .limit(1)
        .to_owned()
}

/// Execute one SeaQuery-built update.
pub(crate) async fn execute_update(connection: &mut PgConnection, statement: UpdateStatement) -> Result<(), MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::state::{RegisteredTenant, TenantId, least_recently_inspected_statement};
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn fair_selection_includes_default_and_orders_never_inspected_first() {
        let sql = least_recently_inspected_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("LEFT JOIN \"postgresql_maintenance_tenants\""), "{sql}");
        assert!(sql.contains("NULLS FIRST"), "{sql}");
        assert!(!sql.contains("is_unnamed_default"), "the default must not be filtered: {sql}");
        assert!(sql.contains("\"tenants\".\"status\" = 1"), "only active tenants are selected, by code: {sql}");
    }

    #[test]
    fn unnamed_default_has_an_explicit_log_label() {
        let tenant = RegisteredTenant {
            id: TenantId(1),
            name: None,
            database: TenantDatabaseName::from("postgres".to_owned()),
        };
        assert_eq!(tenant.log_name(), "<default>");
    }
}
