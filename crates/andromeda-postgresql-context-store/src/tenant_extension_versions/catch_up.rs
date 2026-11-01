//! Reading the tenants behind the server's extension versions from the
//! platform database, for the wake pass to load.
//!
//! One statement reads every active named tenant with no wake pending beside
//! its record, the version the server ships of each extension and the
//! transactions its database committed. `pg_available_extensions` reports the
//! control files installed on the server, which every database of the server
//! shares, so the platform database sees the version each tenant would be
//! updated to without connecting to it. The statement keeps only the tenants
//! with some extension whose recorded spelling differs from the shipped one,
//! which after a catch-up is none of them; which of those are owed a load, and
//! in what order, is decided on the parsed versions.

use crate::{
    schema_contract::{
        server_requirements::{ExtensionPresence, ExtensionVersion},
        versioned_extension::VersionedExtension,
    },
    store_fault::{persistence_failure, undecodable},
    tenant_extension_versions::{
        PostgresTenantExtensionVersions,
        catch_up_rank::{CatchUpCandidate, ExtensionLag, RecordedExtension, catch_up_order},
    },
    tenant_registry::status_code::stored_status,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_schema::tenancy::{TenantExtensionVersions, TenantWakeSchedule, Tenants};
use andromeda_postgresql_seaquery::{introspection::extension_shipped_version, statement::Statement};
use andromeda_tenancy::{
    catch_up::{CatchUpCandidates, CatchUpError},
    status::TenantStatus,
};
use async_trait::async_trait;
use sea_query::{Alias, Cond, Expr, ExprTrait, Func, Iden, JoinType, Query, SelectStatement};
use sqlx::{Row, postgres::PgRow};
use strum::IntoEnumIterator;

/// The `pg_stat_database` statistics view, for the transactions each database
/// committed.
#[derive(Iden, Clone, Copy)]
enum PgStatDatabase {
    /// View name: `pg_stat_database`
    Table,
    /// The database the row counts for
    Datname,
    /// Transactions committed in that database since the statistics were reset
    XactCommit,
}

/// The column the tenant's name is read under.
const NAME_COLUMN: &str = "name";
/// The column the committed transactions are read under.
const COMMITTED_COLUMN: &str = "committed_transactions";

#[async_trait]
impl CatchUpCandidates for PostgresTenantExtensionVersions {
    async fn lagging_tenants(&self) -> Result<Vec<TenantName>, CatchUpError> {
        let rows = lagging_tenants_statement()
            .to_query()
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| CatchUpError::Store(persistence_failure(&error)))?;
        let candidates = rows.iter().map(candidate_from_row).collect::<Result<Vec<CatchUpCandidate>, CatchUpError>>()?;
        Ok(catch_up_order(candidates))
    }
}

/// The alias one extension's record row is joined under.
#[derive(Iden, Clone, Copy)]
enum RecordAlias {
    /// The `PostGIS` record.
    #[iden = "postgis_record"]
    PostGis,
    /// The `TimescaleDB` record.
    #[iden = "timescaledb_record"]
    TimescaleDb,
}

impl RecordAlias {
    /// The alias of `extension`'s record.
    const fn of(extension: VersionedExtension) -> RecordAlias {
        match extension {
            VersionedExtension::PostGis => RecordAlias::PostGis,
            VersionedExtension::TimescaleDb => RecordAlias::TimescaleDb,
        }
    }
}

/// The column saying whether one extension has a record row.
const fn recorded_column(extension: VersionedExtension) -> &'static str {
    match extension {
        VersionedExtension::PostGis => "postgis_recorded",
        VersionedExtension::TimescaleDb => "timescaledb_recorded",
    }
}

/// The column one extension's recorded server version is read under.
const fn reconciled_column(extension: VersionedExtension) -> &'static str {
    match extension {
        VersionedExtension::PostGis => "postgis_reconciled_against",
        VersionedExtension::TimescaleDb => "timescaledb_reconciled_against",
    }
}

/// Every active named tenant with no wake pending and some extension whose
/// record does not spell the shipped version, with what the order needs.
fn lagging_tenants_statement() -> SelectStatement {
    let mut select = Query::select();
    select
        .expr_as(Expr::col((Tenants::Table, Tenants::Name)), Alias::new(NAME_COLUMN))
        .expr_as(
            Func::coalesce([Expr::col((PgStatDatabase::Table, PgStatDatabase::XactCommit)), Expr::val(0_i64)]),
            Alias::new(COMMITTED_COLUMN),
        )
        .from(Tenants::Table)
        .left_join(
            TenantWakeSchedule::Table,
            Expr::col((TenantWakeSchedule::Table, TenantWakeSchedule::TenantKey)).equals((Tenants::Table, Tenants::TenantKey)),
        )
        .left_join(
            PgStatDatabase::Table,
            Expr::col((PgStatDatabase::Table, PgStatDatabase::Datname)).equals((Tenants::Table, Tenants::DatabaseName)),
        );
    let mut unsettled = Cond::any();
    for extension in VersionedExtension::iter() {
        let record = RecordAlias::of(extension);
        select
            .join_as(
                JoinType::LeftJoin,
                TenantExtensionVersions::Table,
                record,
                Expr::col((record, TenantExtensionVersions::TenantKey))
                    .equals((Tenants::Table, Tenants::TenantKey))
                    .and(Expr::col((record, TenantExtensionVersions::Extension)).eq(extension.catalog_name())),
            )
            .expr_as(
                Expr::col((record, TenantExtensionVersions::TenantKey)).is_not_null(),
                Alias::new(recorded_column(extension)),
            )
            .expr_as(
                Expr::col((record, TenantExtensionVersions::InstalledVersion)),
                Alias::new(extension.installed_column()),
            )
            .expr_as(
                Expr::col((record, TenantExtensionVersions::ShippedVersion)),
                Alias::new(reconciled_column(extension)),
            )
            .expr_as(extension_shipped_version(extension.catalog_name()), Alias::new(extension.shipped_column()));
        unsettled = unsettled.add(unsettled_extension(extension));
    }
    select.cond_where(
        Cond::all()
            .add(Expr::col((Tenants::Table, Tenants::Status)).eq(stored_status(TenantStatus::Active).code()))
            .add(Expr::col((Tenants::Table, Tenants::IsUnnamedDefault)).eq(false))
            .add(Expr::col((TenantWakeSchedule::Table, TenantWakeSchedule::WakeAt)).is_null())
            .add(unsettled),
    );
    select
}

/// Whether `extension` may be behind the version the server ships: the server
/// ships one, and the tenant has no record of the extension or a record
/// spelling the shipped version neither as installed nor as the version its
/// last open ran against.
fn unsettled_extension(extension: VersionedExtension) -> Cond {
    let record = RecordAlias::of(extension);
    let shipped = || extension_shipped_version(extension.catalog_name());
    let spelled_unlike_shipped = |column: TenantExtensionVersions| Func::coalesce([Expr::col((record, column)), Expr::val("")]).ne(shipped());
    Cond::all().add(shipped().is_not_null()).add(
        Cond::any().add(Expr::col((record, TenantExtensionVersions::TenantKey)).is_null()).add(
            Cond::all()
                .add(spelled_unlike_shipped(TenantExtensionVersions::InstalledVersion))
                .add(spelled_unlike_shipped(TenantExtensionVersions::ShippedVersion)),
        ),
    )
}

/// One read row as a candidate.
fn candidate_from_row(row: &PgRow) -> Result<CatchUpCandidate, CatchUpError> {
    let name: String = row.try_get(NAME_COLUMN).map_err(|error| CatchUpError::Store(persistence_failure(&error)))?;
    let committed_transactions: i64 = row
        .try_get(COMMITTED_COLUMN)
        .map_err(|error| CatchUpError::Store(persistence_failure(&error)))?;
    Ok(CatchUpCandidate {
        name: TenantName::try_from(name).map_err(|error| CatchUpError::Store(undecodable(&error)))?,
        timescaledb: lag_from_row(row, VersionedExtension::TimescaleDb)?,
        postgis: lag_from_row(row, VersionedExtension::PostGis)?,
        committed_transactions,
    })
}

/// One extension of a read row against the version the server ships.
fn lag_from_row(row: &PgRow, extension: VersionedExtension) -> Result<ExtensionLag, CatchUpError> {
    let column = |name: &str| {
        row.try_get::<Option<String>, _>(name)
            .map_err(|error| CatchUpError::Store(persistence_failure(&error)))
    };
    let recorded: bool = row
        .try_get(recorded_column(extension))
        .map_err(|error| CatchUpError::Store(persistence_failure(&error)))?;
    let shipped = parsed(column(extension.shipped_column())?)?;
    let recorded = if recorded {
        Some(RecordedExtension {
            installed: ExtensionPresence::try_from(column(extension.installed_column())?).map_err(|error| CatchUpError::Store(undecodable(&error)))?,
            reconciled_against: parsed(column(reconciled_column(extension))?)?,
        })
    } else {
        None
    };
    Ok(ExtensionLag { recorded, shipped })
}

/// A version column read as a version, `None` when `NULL`.
fn parsed(raw: Option<String>) -> Result<Option<ExtensionVersion>, CatchUpError> {
    raw.map(|raw| ExtensionVersion::try_from(raw.as_str()))
        .transpose()
        .map_err(|error| CatchUpError::Store(undecodable(&error)))
}

#[cfg(test)]
mod tests {
    use crate::tenant_extension_versions::catch_up::lagging_tenants_statement;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_read_takes_active_named_tenants_with_no_wake_pending() {
        let sql = lagging_tenants_statement().to_string(PostgresQueryBuilder);
        for fragment in [
            r#"LEFT JOIN "tenant_wake_schedule" ON "tenant_wake_schedule"."tenant_key" = "tenants"."tenant_key""#,
            r#""tenants"."status" = 1"#,
            r#""tenants"."is_unnamed_default" = FALSE"#,
            r#""tenant_wake_schedule"."wake_at" IS NULL"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    #[test]
    fn the_read_joins_each_extensions_record_and_the_version_the_server_ships() {
        let sql = lagging_tenants_statement().to_string(PostgresQueryBuilder);
        for fragment in [
            r#"LEFT JOIN "tenant_extension_versions" AS "timescaledb_record" ON "timescaledb_record"."tenant_key" = "tenants"."tenant_key" AND "timescaledb_record"."extension" = 'timescaledb'"#,
            r#"LEFT JOIN "tenant_extension_versions" AS "postgis_record""#,
            r#""timescaledb_record"."tenant_key" IS NOT NULL AS "timescaledb_recorded""#,
            r#""timescaledb_record"."installed_version" AS "timescaledb_version""#,
            r#""timescaledb_record"."shipped_version" AS "timescaledb_reconciled_against""#,
            r#"(SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'timescaledb') AS "timescaledb_shipped_version""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    #[test]
    fn the_read_counts_each_databases_committed_transactions() {
        let sql = lagging_tenants_statement().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"LEFT JOIN "pg_stat_database" ON "pg_stat_database"."datname" = "tenants"."database_name""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"COALESCE("pg_stat_database"."xact_commit", 0) AS "committed_transactions""#),
            "{sql}"
        );
    }

    #[test]
    fn the_read_keeps_a_tenant_without_a_record_or_whose_record_spells_neither_shipped_version() {
        let sql = lagging_tenants_statement().to_string(PostgresQueryBuilder);
        for fragment in [
            r#""timescaledb_record"."tenant_key" IS NULL"#,
            r#"COALESCE("timescaledb_record"."installed_version", '') <> (SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'timescaledb')"#,
            r#"COALESCE("timescaledb_record"."shipped_version", '') <> (SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'timescaledb')"#,
            r#"(SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'postgis') IS NOT NULL"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }
}
