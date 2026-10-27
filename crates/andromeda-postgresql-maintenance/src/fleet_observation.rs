//! What one platform-side statement reveals about every database at once.
//!
//! `pg_database` and `pg_stat_database` are cluster-wide, so the platform
//! connection reads the freeze ages and change counters of every database
//! that accepts connections without connecting to any of them. The registry
//! only labels them: an active tenant's database is one tenant selection
//! orders by, and every other database (the server's own, a suspended
//! tenant's, one no registry row names) is reported without a visit. The pass
//! takes this observation before it selects a tenant; the telemetry reports
//! the whole cluster. The platform database's change counters leave out the
//! platform tables, so the controller's own bookkeeping does not make the
//! default tenant due.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    freeze_policy::{FreezeAge, FreezePressure, freeze_pressure},
    platform_relations::platform_tuple_changes,
    state::TenantKey,
    unvisited_database::{DatabaseName, UnvisitedDatabase, UnvisitedRegistration},
};
use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_schema::tenancy::Tenants;
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::current_setting,
    maintenance::{multixact_age, transaction_age},
    pg_type::PostgresType,
    setting::Setting,
    statement::Statement,
};
use andromeda_telemetry::maintenance::{FreezePressureLevel, record_fleet_freeze};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement};
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

/// A tenant database's cumulative change counters as `pg_stat_database`
/// reports them, without the writes to the platform tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChangeCounters {
    /// `tup_inserted + tup_updated + tup_deleted` since the statistics were
    /// last reset, less the platform tables' share.
    pub(crate) tuple_changes: u64,
    /// When the database's statistics were last reset, `None` if never.
    pub(crate) stats_reset: Option<DateTime<Utc>>,
}

impl ChangeCounters {
    /// The counters of a database whose `database_changes` include the
    /// `platform_changes` made to the platform tables, zero outside the
    /// platform database.
    ///
    /// The database total accumulates every table's counters, so it never
    /// holds less than the platform tables' share except for a read that
    /// lands between two flushes; that read counts no change.
    pub(crate) const fn of_tenant(database_changes: u64, platform_changes: u64, stats_reset: Option<DateTime<Utc>>) -> ChangeCounters {
        ChangeCounters {
            tuple_changes: database_changes.saturating_sub(platform_changes),
            stats_reset,
        }
    }
}

/// What the fleet statement saw of one active tenant's database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DatabaseObservation {
    /// Platform row of the tenant the database belongs to.
    pub(crate) tenant: TenantKey,
    /// Its freeze ages.
    pub(crate) freeze: DatabaseFreeze,
    /// Its change counters.
    pub(crate) changes: ChangeCounters,
}

/// One fleet row, sorted by whether a visit reaches its database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FleetEntry {
    /// An active tenant's database, which tenant selection may visit.
    Visited(DatabaseObservation),
    /// Any other database that accepts connections.
    Unvisited(UnvisitedDatabase),
}

/// What one fleet row carries before it is sorted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FleetRow {
    /// The database's name.
    pub(crate) name: DatabaseName,
    /// The registry row naming the database, `None` when none does.
    pub(crate) tenant: Option<(TenantKey, TenantStatusCode)>,
    /// Its freeze ages.
    pub(crate) freeze: DatabaseFreeze,
    /// Its change counters.
    pub(crate) changes: ChangeCounters,
}

impl FleetRow {
    /// Sort the row: only an active tenant's database is visited.
    pub(crate) fn entry(self) -> FleetEntry {
        let registration = match self.tenant {
            Some((tenant, TenantStatusCode::Active)) => {
                return FleetEntry::Visited(DatabaseObservation {
                    tenant,
                    freeze: self.freeze,
                    changes: self.changes,
                });
            }
            Some((_, TenantStatusCode::Suspended)) => UnvisitedRegistration::Suspended,
            Some((_, TenantStatusCode::Provisional)) => UnvisitedRegistration::Provisional,
            None => UnvisitedRegistration::Unregistered,
        };
        FleetEntry::Unvisited(UnvisitedDatabase {
            name: self.name,
            registration,
            freeze: self.freeze,
        })
    }
}

/// One observation of every database that accepts connections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FleetObservation {
    /// One entry per active tenant whose database exists.
    databases: Vec<DatabaseObservation>,
    /// Every other database.
    unvisited: Vec<UnvisitedDatabase>,
}

impl FleetObservation {
    /// The observation of one tenant's database, `None` when the database did
    /// not exist when the fleet was observed.
    pub(crate) fn database(&self, tenant: TenantKey) -> Option<DatabaseObservation> {
        self.databases.iter().copied().find(|database| database.tenant == tenant)
    }

    /// The freeze ages of every database the fleet holds, visited or not.
    fn every_freeze(&self) -> impl Iterator<Item = DatabaseFreeze> + '_ {
        self.databases
            .iter()
            .map(|database| database.freeze)
            .chain(self.unvisited.iter().map(|database| database.freeze))
    }

    /// Report the cluster's oldest ages and its databases per pressure level.
    /// An empty fleet reports ages of zero.
    pub(crate) fn record_telemetry(&self) {
        let oldest = |age: fn(DatabaseFreeze) -> u64| self.every_freeze().map(age).max().unwrap_or_default();
        let levels = self.every_freeze().map(|freeze| metric_level(freeze.pressure())).collect::<Vec<_>>();
        record_fleet_freeze(oldest(|freeze| freeze.transaction.age), oldest(|freeze| freeze.multixact.age), &levels);
    }

    /// Log what every database no visit reaches warrants.
    pub(crate) fn report_unvisited(&self, tenant_database_prefix: &DbNamePrefix) {
        for database in &self.unvisited {
            database.report(tenant_database_prefix);
        }
    }
}

/// Read every database's freeze ages and change counters.
pub(crate) async fn observe_fleet(connection: &mut PgConnection) -> Result<FleetObservation, MaintenanceError> {
    let rows = fleet_observation_statement().to_query().fetch_all(connection).await?;
    let mut fleet = FleetObservation {
        databases: Vec::new(),
        unvisited: Vec::new(),
    };
    for row in &rows {
        match fleet_row_of(row)?.entry() {
            FleetEntry::Visited(database) => fleet.databases.push(database),
            FleetEntry::Unvisited(database) => fleet.unvisited.push(database),
        }
    }
    Ok(fleet)
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
    /// `tup_inserted + tup_updated + tup_deleted`.
    TupleChanges,
    /// The platform tables' share of `TupleChanges`, zero outside the
    /// platform database.
    PlatformTupleChanges,
}

/// The registry and catalogue labels the fleet statement projects.
#[derive(Clone, Copy, Iden)]
enum FleetLabel {
    /// `pg_database.datname`.
    DatabaseName,
    /// `tenants.status`, NULL for an unregistered database.
    TenantStatus,
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

/// Every database that accepts connections, with its `pg_stat_database` row
/// and the registry row naming it, if any.
///
/// The freeze maxima are server-wide settings, read on every row so one
/// statement carries everything the classification needs. Every database has
/// a `pg_stat_database` row, so the inner join drops none. A database that
/// refuses connections (`template0`) is frozen and never written.
fn fleet_observation_statement() -> SelectStatement {
    Query::select()
        .expr_as(catalog_column("pg_database", "datname"), FleetLabel::DatabaseName)
        .column((Tenants::Table, Tenants::TenantKey))
        .expr_as(Expr::col((Tenants::Table, Tenants::Status)), FleetLabel::TenantStatus)
        .expr_as(transaction_age(catalog_column("pg_database", "datfrozenxid")), FleetColumn::TransactionIdAge)
        .expr_as(
            cast(current_setting(Setting::AutovacuumFreezeMaxAge), PostgresType::Bigint),
            FleetColumn::TransactionFreezeMaximum,
        )
        .expr_as(multixact_age(catalog_column("pg_database", "datminmxid")), FleetColumn::MultixactIdAge)
        .expr_as(
            cast(current_setting(Setting::AutovacuumMultixactFreezeMaxAge), PostgresType::Bigint),
            FleetColumn::MultixactFreezeMaximum,
        )
        .expr_as(
            catalog_column("pg_stat_database", "tup_inserted")
                .add(catalog_column("pg_stat_database", "tup_updated"))
                .add(catalog_column("pg_stat_database", "tup_deleted")),
            FleetColumn::TupleChanges,
        )
        .expr_as(
            platform_tuple_changes(catalog_column("pg_database", "datname")),
            FleetColumn::PlatformTupleChanges,
        )
        .expr_as(catalog_column("pg_stat_database", "stats_reset"), FleetInstant::StatsReset)
        .from(Alias::new("pg_database"))
        .join(
            JoinType::InnerJoin,
            Alias::new("pg_stat_database"),
            catalog_column("pg_stat_database", "datid").equals((Alias::new("pg_database"), Alias::new("oid"))),
        )
        .join(
            JoinType::LeftJoin,
            Tenants::Table,
            Expr::col((Tenants::Table, Tenants::DatabaseName)).equals((Alias::new("pg_database"), Alias::new("datname"))),
        )
        .and_where(catalog_column("pg_database", "datallowconn"))
        .to_owned()
}

/// Decode one fleet row.
fn fleet_row_of(row: &PgRow) -> Result<FleetRow, MaintenanceError> {
    let counter = |column: FleetColumn| -> Result<u64, MaintenanceError> {
        let kind = match column {
            FleetColumn::TransactionIdAge | FleetColumn::MultixactIdAge => CatalogCounter::DatabaseFreezeAge,
            FleetColumn::TransactionFreezeMaximum | FleetColumn::MultixactFreezeMaximum => CatalogCounter::FreezeMaxAge,
            FleetColumn::TupleChanges | FleetColumn::PlatformTupleChanges => CatalogCounter::TupleChanges,
        };
        non_negative(row.try_get(column.to_string().as_str())?, kind)
    };
    let key = row.try_get::<Option<i64>, _>(Tenants::TenantKey.to_string().as_str())?;
    let status = row
        .try_get::<Option<i16>, _>(FleetLabel::TenantStatus.to_string().as_str())?
        .map(TenantStatusCode::try_from)
        .transpose()?;
    Ok(FleetRow {
        name: DatabaseName(row.try_get(FleetLabel::DatabaseName.to_string().as_str())?),
        tenant: key.zip(status).map(|(key, status)| (TenantKey(key), status)),
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
        changes: ChangeCounters::of_tenant(
            counter(FleetColumn::TupleChanges)?,
            counter(FleetColumn::PlatformTupleChanges)?,
            row.try_get(FleetInstant::StatsReset.to_string().as_str())?,
        ),
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
    use crate::{
        fleet_observation::{ChangeCounters, DatabaseFreeze, DatabaseObservation, FleetEntry, FleetRow, fleet_observation_statement},
        freeze_policy::FreezeAge,
        state::TenantKey,
        unvisited_database::{DatabaseName, UnvisitedDatabase, UnvisitedRegistration},
    };
    use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    const FREEZE: DatabaseFreeze = DatabaseFreeze {
        transaction: FreezeAge {
            age: 300_000_000,
            freeze_max_age: 200_000_000,
        },
        multixact: FreezeAge {
            age: 0,
            freeze_max_age: 400_000_000,
        },
    };

    const CHANGES: ChangeCounters = ChangeCounters {
        tuple_changes: 42,
        stats_reset: None,
    };

    fn row(name: &str, tenant: Option<(TenantKey, TenantStatusCode)>) -> FleetRow {
        FleetRow {
            name: DatabaseName(name.to_owned()),
            tenant,
            freeze: FREEZE,
            changes: CHANGES,
        }
    }

    #[test]
    fn only_an_active_tenants_database_is_visited() {
        assert_eq!(
            row("andromeda_tenant_acme_0badc0de", Some((TenantKey(7), TenantStatusCode::Active))).entry(),
            FleetEntry::Visited(DatabaseObservation {
                tenant: TenantKey(7),
                freeze: FREEZE,
                changes: CHANGES,
            })
        );
        assert_eq!(
            row("andromeda_tenant_acme_0badc0de", Some((TenantKey(7), TenantStatusCode::Suspended))).entry(),
            FleetEntry::Unvisited(UnvisitedDatabase {
                name: DatabaseName("andromeda_tenant_acme_0badc0de".to_owned()),
                registration: UnvisitedRegistration::Suspended,
                freeze: FREEZE,
            })
        );
        assert_eq!(
            row("andromeda_tenant_acme_0badc0de", Some((TenantKey(7), TenantStatusCode::Provisional))).entry(),
            FleetEntry::Unvisited(UnvisitedDatabase {
                name: DatabaseName("andromeda_tenant_acme_0badc0de".to_owned()),
                registration: UnvisitedRegistration::Provisional,
                freeze: FREEZE,
            })
        );
        assert_eq!(
            row("template1", None).entry(),
            FleetEntry::Unvisited(UnvisitedDatabase {
                name: DatabaseName("template1".to_owned()),
                registration: UnvisitedRegistration::Unregistered,
                freeze: FREEZE,
            })
        );
    }

    #[test]
    fn every_database_that_accepts_connections_is_observed_and_the_registry_only_labels_it() {
        let sql = fleet_observation_statement().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"LEFT JOIN "tenants" ON "tenants"."database_name" = "pg_database"."datname""#),
            "{sql}"
        );
        assert!(sql.contains(r#"WHERE "pg_database"."datallowconn""#), "{sql}");
        assert!(sql.contains(r#""pg_database"."datname" AS "database_name""#), "{sql}");
        assert!(sql.contains(r#""tenants"."status" AS "tenant_status""#), "{sql}");
        assert!(!sql.contains(r#""tenants"."status" = "#), "no status filters a database out: {sql}");
    }

    #[test]
    fn fleet_observation_statement_reads_database_and_multixact_age() {
        let sql = fleet_observation_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "pg_database""#), "{sql}");
        assert!(sql.contains(r#"age("pg_database"."datfrozenxid")::bigint"#), "{sql}");
        assert!(sql.contains(r#"mxid_age("pg_database"."datminmxid")::bigint"#), "{sql}");
        assert!(sql.contains("current_setting('autovacuum_freeze_max_age')::bigint"), "{sql}");
        assert!(sql.contains("current_setting('autovacuum_multixact_freeze_max_age')::bigint"), "{sql}");
        assert!(sql.contains(r#""tenants"."database_name" = "pg_database"."datname""#), "{sql}");
    }

    #[test]
    fn fleet_observation_statement_reads_tuple_changes_and_stats_reset() {
        let sql = fleet_observation_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"JOIN "pg_stat_database""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_database"."datid" = "pg_database"."oid""#), "{sql}");
        assert!(
            sql.contains(r#""pg_stat_database"."tup_inserted" + "pg_stat_database"."tup_updated" + "pg_stat_database"."tup_deleted""#),
            "inserts leave planner statistics stale, so they count as churn: {sql}"
        );
        assert!(sql.contains(r#""pg_stat_database"."stats_reset""#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_database""#), "both catalogs are read by one statement: {sql}");
        assert!(
            sql.contains(r#"CASE WHEN ("pg_database"."datname" = current_database()) THEN"#),
            "the platform tables are read for the platform database's row: {sql}"
        );
    }

    #[test]
    fn the_platform_tables_share_is_left_out_of_the_change_counters() {
        assert_eq!(ChangeCounters::of_tenant(1_040, 340, None).tuple_changes, 700);
        assert_eq!(ChangeCounters::of_tenant(1_040, 0, None).tuple_changes, 1_040);
        assert_eq!(ChangeCounters::of_tenant(300, 340, None).tuple_changes, 0);
    }
}
