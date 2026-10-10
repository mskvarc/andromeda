//! What one platform-side statement reveals about every tenant database at
//! once.
//!
//! `pg_database` and `pg_stat_database` are cluster-wide, so the platform
//! connection reads the freeze ages and change counters of every registered
//! database without connecting to any of them. The pass takes this
//! observation before it selects a tenant: it is what tenant selection orders
//! by and the fleet-wide picture the telemetry reports.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    policy::{FreezeAge, FreezePressure, freeze_pressure},
    state::TenantKey,
};
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_schema::tenancy::Tenants;
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::current_setting,
    maintenance::{multixact_age, transaction_age},
    pg_type::PostgresType,
};
use andromeda_telemetry::maintenance::{FreezePressureLevel, record_fleet_freeze};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Iden, JoinType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row, postgres::PgRow};

/// The freeze ages of one database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DatabaseFreeze {
    /// Transaction ID age and its freeze maximum.
    pub(crate) transaction: FreezeAge,
    /// Multixact ID age and its freeze maximum.
    pub(crate) multixact: FreezeAge,
}

impl DatabaseFreeze {
    /// The database's freeze pressure, the more urgent of its two ID spaces.
    pub(crate) fn pressure(self) -> FreezePressure {
        freeze_pressure(self.transaction, self.multixact)
    }
}

/// A database's cumulative change counters as `pg_stat_database` reports
/// them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChangeCounters {
    /// `tup_updated + tup_deleted` since the statistics were last reset.
    pub(crate) tuple_changes: u64,
    /// When the database's statistics were last reset, `None` if never.
    pub(crate) stats_reset: Option<DateTime<Utc>>,
}

/// What the fleet statement saw of one registered tenant database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DatabaseObservation {
    /// Platform row of the tenant the database belongs to.
    pub(crate) tenant: TenantKey,
    /// Its freeze ages.
    pub(crate) freeze: DatabaseFreeze,
    /// Its change counters.
    pub(crate) changes: ChangeCounters,
}

/// One observation of every active tenant database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FleetObservation {
    /// One entry per active tenant whose database exists.
    databases: Vec<DatabaseObservation>,
}

impl FleetObservation {
    /// The observation of one tenant's database, `None` when the database did
    /// not exist when the fleet was observed.
    pub(crate) fn database(&self, tenant: TenantKey) -> Option<DatabaseObservation> {
        self.databases.iter().copied().find(|database| database.tenant == tenant)
    }

    /// Report the fleet's oldest ages and its databases per pressure level.
    /// An empty fleet reports ages of zero.
    pub(crate) fn record_telemetry(&self) {
        let oldest = |age: fn(&DatabaseObservation) -> u64| self.databases.iter().map(age).max().unwrap_or_default();
        let levels = self
            .databases
            .iter()
            .map(|database| metric_level(database.freeze.pressure()))
            .collect::<Vec<_>>();
        record_fleet_freeze(
            oldest(|database| database.freeze.transaction.age),
            oldest(|database| database.freeze.multixact.age),
            &levels,
        );
    }
}

/// Read every active tenant database's freeze ages and change counters.
pub(crate) async fn observe_fleet(connection: &mut PgConnection) -> Result<FleetObservation, MaintenanceError> {
    let (sql, values) = fleet_observation_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    Ok(FleetObservation {
        databases: rows.iter().map(database_observation_of).collect::<Result<_, _>>()?,
    })
}

/// The columns the fleet statement projects.
#[derive(Clone, Copy, Iden)]
enum FleetColumn {
    /// `age(datfrozenxid)`.
    TransactionIdAge,
    /// `autovacuum_freeze_max_age`.
    TransactionFreezeMaximum,
    /// `mxid_age(datminmxid)`.
    MultixactIdAge,
    /// `autovacuum_multixact_freeze_max_age`.
    MultixactFreezeMaximum,
    /// `tup_updated + tup_deleted`.
    TupleChanges,
}

/// The instant the fleet statement projects.
#[derive(Clone, Copy, Iden)]
enum FleetInstant {
    /// `stats_reset`.
    StatsReset,
}

/// A column of one of the two cluster-wide catalogs the statement reads.
fn catalog_column(catalog: &'static str, column: &'static str) -> Expr {
    Expr::col((Alias::new(catalog), Alias::new(column)))
}

/// `pg_database` and its `pg_stat_database` row joined to the active
/// registry rows by database name.
///
/// The freeze maxima are server-wide settings, read on every row so one
/// statement carries everything the classification needs. Every database has
/// a `pg_stat_database` row, so the inner join drops none.
fn fleet_observation_statement() -> SelectStatement {
    Query::select()
        .column((Tenants::Table, Tenants::TenantKey))
        .expr_as(transaction_age(catalog_column("pg_database", "datfrozenxid")), FleetColumn::TransactionIdAge)
        .expr_as(
            cast(current_setting("autovacuum_freeze_max_age"), PostgresType::Bigint),
            FleetColumn::TransactionFreezeMaximum,
        )
        .expr_as(multixact_age(catalog_column("pg_database", "datminmxid")), FleetColumn::MultixactIdAge)
        .expr_as(
            cast(current_setting("autovacuum_multixact_freeze_max_age"), PostgresType::Bigint),
            FleetColumn::MultixactFreezeMaximum,
        )
        .expr_as(
            catalog_column("pg_stat_database", "tup_updated").add(catalog_column("pg_stat_database", "tup_deleted")),
            FleetColumn::TupleChanges,
        )
        .expr_as(catalog_column("pg_stat_database", "stats_reset"), FleetInstant::StatsReset)
        .from(Alias::new("pg_database"))
        .join(
            JoinType::InnerJoin,
            Alias::new("pg_stat_database"),
            catalog_column("pg_stat_database", "datid").equals((Alias::new("pg_database"), Alias::new("oid"))),
        )
        .join(
            JoinType::InnerJoin,
            Tenants::Table,
            Expr::col((Tenants::Table, Tenants::DatabaseName)).equals((Alias::new("pg_database"), Alias::new("datname"))),
        )
        .and_where(Expr::col((Tenants::Table, Tenants::Status)).eq(TenantStatusCode::Active.code()))
        .to_owned()
}

/// Decode one fleet row.
fn database_observation_of(row: &PgRow) -> Result<DatabaseObservation, MaintenanceError> {
    let counter = |column: FleetColumn| -> Result<u64, MaintenanceError> {
        let kind = match column {
            FleetColumn::TransactionIdAge | FleetColumn::MultixactIdAge => CatalogCounter::DatabaseFreezeAge,
            FleetColumn::TransactionFreezeMaximum | FleetColumn::MultixactFreezeMaximum => CatalogCounter::FreezeMaxAge,
            FleetColumn::TupleChanges => CatalogCounter::TupleChanges,
        };
        non_negative(row.try_get(column.to_string().as_str())?, kind)
    };
    Ok(DatabaseObservation {
        tenant: TenantKey(row.try_get(Tenants::TenantKey.to_string().as_str())?),
        freeze: DatabaseFreeze {
            transaction: FreezeAge {
                age: counter(FleetColumn::TransactionIdAge)?,
                freeze_max_age: counter(FleetColumn::TransactionFreezeMaximum)?,
            },
            multixact: FreezeAge {
                age: counter(FleetColumn::MultixactIdAge)?,
                freeze_max_age: counter(FleetColumn::MultixactFreezeMaximum)?,
            },
        },
        changes: ChangeCounters {
            tuple_changes: counter(FleetColumn::TupleChanges)?,
            stats_reset: row.try_get(FleetInstant::StatsReset.to_string().as_str())?,
        },
    })
}

/// Map a pressure to the telemetry crate's stable label domain.
const fn metric_level(pressure: FreezePressure) -> FreezePressureLevel {
    match pressure {
        FreezePressure::Normal => FreezePressureLevel::Normal,
        FreezePressure::Overdue => FreezePressureLevel::Overdue,
        FreezePressure::Critical => FreezePressureLevel::Critical,
    }
}

#[cfg(test)]
mod tests {
    use crate::fleet_observation::fleet_observation_statement;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn fleet_observation_statement_reads_database_and_multixact_age() {
        let sql = fleet_observation_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "pg_database""#), "{sql}");
        assert!(sql.contains(r#"age("pg_database"."datfrozenxid")::bigint"#), "{sql}");
        assert!(sql.contains(r#"mxid_age("pg_database"."datminmxid")::bigint"#), "{sql}");
        assert!(sql.contains("current_setting('autovacuum_freeze_max_age')::bigint"), "{sql}");
        assert!(sql.contains("current_setting('autovacuum_multixact_freeze_max_age')::bigint"), "{sql}");
        assert!(sql.contains(r#""tenants"."database_name" = "pg_database"."datname""#), "{sql}");
        assert!(sql.contains(r#""tenants"."status" = 1"#), "only active tenants are observed, by code: {sql}");
    }

    #[test]
    fn fleet_observation_statement_reads_tuple_changes_and_stats_reset() {
        let sql = fleet_observation_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"JOIN "pg_stat_database""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_database"."datid" = "pg_database"."oid""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_database"."tup_updated" + "pg_stat_database"."tup_deleted""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_database"."stats_reset""#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_database""#), "both catalogs are read by one statement: {sql}");
    }
}
