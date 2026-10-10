//! The cluster's run of passes that found every autovacuum worker slot busy.
//!
//! Every broker replica runs passes, one at a time under the fleet lock, so
//! the run is kept in the platform database where the next pass reads it
//! whichever replica takes it.

use crate::{catalog_counter::CatalogCounter, error::MaintenanceError};
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceFleet;
use sea_query::{InsertStatement, OnConflict, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgConnection;

/// Read how many consecutive passes found every worker slot busy, zero
/// before the first pass recorded any.
pub(crate) async fn saturated_passes(connection: &mut PgConnection) -> Result<u32, MaintenanceError> {
    let (sql, values) = saturated_passes_statement().build_sqlx(PostgresQueryBuilder);
    let recorded = sqlx::query_scalar_with::<_, i32, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(connection)
        .await?;
    recorded.map_or(Ok(0), |passes| {
        u32::try_from(passes).map_err(|_| MaintenanceError::NegativeCatalogValue {
            counter: CatalogCounter::SaturatedPasses,
            value: i64::from(passes),
        })
    })
}

/// Record how many consecutive passes found every worker slot busy.
pub(crate) async fn record_saturated_passes(connection: &mut PgConnection, passes: u32) -> Result<(), MaintenanceError> {
    let (sql, values) = saturated_passes_upsert(passes)?.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

/// The recorded run, no row before the first pass.
fn saturated_passes_statement() -> SelectStatement {
    Query::select()
        .column(PostgresqlMaintenanceFleet::AutovacuumSaturatedPasses)
        .from(PostgresqlMaintenanceFleet::Table)
        .to_owned()
}

/// Write the run into the one fleet row, creating it on the first pass.
fn saturated_passes_upsert(passes: u32) -> Result<InsertStatement, MaintenanceError> {
    let stored = i32::try_from(passes).map_err(|_| MaintenanceError::CatalogValueOverflow {
        counter: CatalogCounter::SaturatedPasses,
        value: u64::from(passes),
    })?;
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceFleet::Table)
        .columns([PostgresqlMaintenanceFleet::Singleton, PostgresqlMaintenanceFleet::AutovacuumSaturatedPasses])
        .values([true.into(), stored.into()])?
        .on_conflict(
            OnConflict::column(PostgresqlMaintenanceFleet::Singleton)
                .update_column(PostgresqlMaintenanceFleet::AutovacuumSaturatedPasses)
                .to_owned(),
        );
    Ok(statement)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::MaintenanceError,
        state::autovacuum_saturation::{saturated_passes_statement, saturated_passes_upsert},
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_saturation_run_is_read_from_the_one_fleet_row() {
        let sql = saturated_passes_statement().to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "autovacuum_saturated_passes" FROM "postgresql_maintenance_fleet""#);
    }

    #[test]
    fn the_saturation_run_is_written_over_the_one_fleet_row() -> Result<(), MaintenanceError> {
        let sql = saturated_passes_upsert(2)?.to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"INSERT INTO "postgresql_maintenance_fleet" ("singleton", "autovacuum_saturated_passes") VALUES (TRUE, 2) ON CONFLICT ("singleton") DO UPDATE SET "autovacuum_saturated_passes" = "excluded"."autovacuum_saturated_passes""#
        );
        Ok(())
    }
}
