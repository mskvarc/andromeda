//! The durable evidence of one tenant's inspection.

use crate::{
    catalog_counter::{CatalogCounter, non_negative, persisted_count},
    error::MaintenanceError,
    evidence::inspection_result::InspectionResult,
    fleet_observation::ChangeCounters,
    state::{RegisteredTenant, TenantKey, execute_update},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_schema::{maintenance::PostgresqlMaintenanceTenants, tenancy::Tenants};
use andromeda_tenancy::database_name::TenantDatabaseName;
use chrono::{DateTime, Utc};
use sea_query::{
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    JoinType,
    OnConflict,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    UpdateStatement,
    error::Error as SeaQueryError,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row, postgres::PgRow};

/// One active registry row with what its inspections recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InspectionRecord {
    /// The registered tenant.
    pub(crate) tenant: RegisteredTenant,
    /// Most recent inspection mark, `None` before the first visit.
    pub(crate) last_inspected_at: Option<DateTime<Utc>>,
    /// The database's change counters when its last visit started, `None`
    /// before a visit observed the database.
    pub(crate) recorded: Option<ChangeCounters>,
}

/// Read every active registry row, the unnamed default included, with its
/// recorded inspection state.
pub(crate) async fn inspection_records(connection: &mut PgConnection) -> Result<Vec<InspectionRecord>, MaintenanceError> {
    let (sql, values) = inspection_records_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter().map(inspection_record_of).collect()
}

/// Mark the start of a tenant's visit, recording the change counters the
/// fleet observation saw so the next selection measures churn from them. A
/// database the observation did not see keeps its earlier counters.
pub(crate) async fn mark_started(connection: &mut PgConnection, tenant: &RegisteredTenant, observed: Option<ChangeCounters>) -> Result<(), MaintenanceError> {
    ensure_tenant_state(connection, tenant).await?;
    execute_update(connection, started_update(tenant, observed)?).await
}

/// The start marker's update, carrying the observed counters when there are
/// any.
fn started_update(tenant: &RegisteredTenant, observed: Option<ChangeCounters>) -> Result<UpdateStatement, MaintenanceError> {
    let mut statement = inspection_update(tenant, InspectionResult::Started);
    if let Some(observed) = observed {
        statement
            .value(
                PostgresqlMaintenanceTenants::LastTupleChanges,
                persisted_count(observed.tuple_changes, CatalogCounter::TupleChanges)?,
            )
            .value(PostgresqlMaintenanceTenants::LastStatsReset, observed.stats_reset);
    }
    Ok(statement)
}

/// Persist that a tenant received its turn even when it was unavailable.
pub(crate) async fn mark_inspected(connection: &mut PgConnection, tenant: &RegisteredTenant, result: InspectionResult) -> Result<(), MaintenanceError> {
    ensure_tenant_state(connection, tenant).await?;
    execute_update(connection, inspection_update(tenant, result)).await
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

/// The update stamping one inspection result; a result concluding the pass
/// also clears the reachability failures.
fn inspection_update(tenant: &RegisteredTenant, result: InspectionResult) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(PostgresqlMaintenanceTenants::Table)
        .value(PostgresqlMaintenanceTenants::LastInspectedAt, Expr::current_timestamp())
        .value(PostgresqlMaintenanceTenants::LastResult, result.code().code())
        .and_where(Expr::col(PostgresqlMaintenanceTenants::TenantKey).eq(tenant.key.0));
    if result.concludes_the_pass() {
        statement.value(PostgresqlMaintenanceTenants::ConsecutiveFailures, 0);
    }
    statement
}

/// Every active registry row beside its maintenance state, if any.
fn inspection_records_statement() -> SelectStatement {
    Query::select()
        .columns([
            (Tenants::Table, Tenants::TenantKey),
            (Tenants::Table, Tenants::Name),
            (Tenants::Table, Tenants::DatabaseName),
        ])
        .columns([
            (PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::LastInspectedAt),
            (PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::LastTupleChanges),
            (PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::LastStatsReset),
        ])
        .from(Tenants::Table)
        .join(
            JoinType::LeftJoin,
            PostgresqlMaintenanceTenants::Table,
            Expr::col((PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::TenantKey)).equals((Tenants::Table, Tenants::TenantKey)),
        )
        .and_where(Expr::col((Tenants::Table, Tenants::Status)).eq(TenantStatusCode::Active.code()))
        .to_owned()
}

/// Decode one registry row and its recorded inspection state.
fn inspection_record_of(row: &PgRow) -> Result<InspectionRecord, MaintenanceError> {
    let raw_name = row.try_get::<Option<String>, _>(Tenants::Name.to_string().as_str())?;
    let tuple_changes = row.try_get::<Option<i64>, _>(PostgresqlMaintenanceTenants::LastTupleChanges.to_string().as_str())?;
    let stats_reset = row.try_get(PostgresqlMaintenanceTenants::LastStatsReset.to_string().as_str())?;
    Ok(InspectionRecord {
        tenant: RegisteredTenant {
            key: TenantKey(row.try_get(Tenants::TenantKey.to_string().as_str())?),
            name: raw_name.map(TenantName::try_from).transpose()?,
            database: TenantDatabaseName::from(row.try_get::<String, _>(Tenants::DatabaseName.to_string().as_str())?),
        },
        last_inspected_at: row.try_get(PostgresqlMaintenanceTenants::LastInspectedAt.to_string().as_str())?,
        recorded: tuple_changes
            .map(|changes| {
                Ok::<_, MaintenanceError>(ChangeCounters {
                    tuple_changes: non_negative(changes, CatalogCounter::TupleChanges)?,
                    stats_reset,
                })
            })
            .transpose()?,
    })
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

#[cfg(test)]
mod tests {
    use crate::{
        error::MaintenanceError,
        fleet_observation::ChangeCounters,
        state::{
            RegisteredTenant,
            TenantKey,
            tenant_inspection::{inspection_records_statement, started_update},
        },
    };
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;

    fn default_tenant() -> RegisteredTenant {
        RegisteredTenant {
            key: TenantKey(3),
            name: None,
            database: TenantDatabaseName::from("postgres".to_owned()),
        }
    }

    #[test]
    fn inspection_records_include_the_default_and_tenants_never_inspected() {
        let sql = inspection_records_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"LEFT JOIN "postgresql_maintenance_tenants""#), "{sql}");
        assert!(sql.contains(r#""postgresql_maintenance_tenants"."last_tuple_changes""#), "{sql}");
        assert!(sql.contains(r#""postgresql_maintenance_tenants"."last_stats_reset""#), "{sql}");
        assert!(!sql.contains("is_unnamed_default"), "the default must not be filtered: {sql}");
        assert!(sql.contains(r#""tenants"."status" = 1"#), "only active tenants are read, by code: {sql}");
    }

    #[test]
    fn the_start_marker_records_the_observed_counters_and_keeps_the_failure_count() -> Result<(), MaintenanceError> {
        let observed = ChangeCounters {
            tuple_changes: 42,
            stats_reset: Some(Utc::now()),
        };
        let sql = started_update(&default_tenant(), Some(observed))?.to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""last_tuple_changes" = 42"#), "{sql}");
        assert!(sql.contains(r#""last_stats_reset" = '"#), "{sql}");
        assert!(!sql.contains("consecutive_failures"), "a start never clears the failure count: {sql}");
        Ok(())
    }

    #[test]
    fn a_start_without_an_observed_database_keeps_the_recorded_counters() -> Result<(), MaintenanceError> {
        let sql = started_update(&default_tenant(), None)?.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("last_tuple_changes"), "{sql}");
        assert!(!sql.contains("last_stats_reset"), "{sql}");
        Ok(())
    }
}
