//! The durable record of every repack whose objects are in a tenant database.
//!
//! A run writes its row before its setup commits and moves it on at each
//! phase, and the row is deleted once the run's objects are dropped. A row
//! that outlives its run, because the broker running it stopped, names the
//! table whose objects recovery drops. One run runs at a time across the
//! cluster, under the repack fleet lock, so while nobody holds that lock
//! every row belongs to a run that is no longer running.

use crate::{
    branch::BranchTarget,
    catalog_counter::{CatalogCounter, object_id},
    error::MaintenanceError,
    evidence::target_code::{stored_target, target_of},
    repack::phase::RepackPhase,
    state::{
        RegisteredTenant,
        TenantKey,
        branch_rows::{branch_of_stored_key, stored_snapshot_key},
    },
};
use andromeda_postgresql_codes::{maintenance_repack_phase::MaintenanceRepackPhaseCode, maintenance_target::MaintenanceTargetCode};
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceRepackRuns;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use sea_query::{
    DeleteStatement,
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    OnConflict,
    Order,
    Query,
    SelectStatement,
    UpdateStatement,
    error::Error as SeaQueryError,
};
use sqlx::{PgConnection, Row, postgres::PgRow};

/// One recorded run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RepackRunRow {
    /// The tenant whose database holds the run's objects.
    pub(crate) tenant: TenantKey,
    /// The repack target, in its branch.
    pub(crate) row: BranchTarget,
    /// The table's OID in the tenant database.
    pub(crate) relid: u32,
    /// The phase the run reached.
    pub(crate) phase: RepackPhase,
    /// When the run installed its trigger.
    pub(crate) started_at: DateTime<Utc>,
}

/// Record a run of `row` on the table `relid` reaching its setup.
pub(crate) async fn record_start(connection: &mut PgConnection, tenant: &RegisteredTenant, row: BranchTarget, relid: u32) -> Result<(), MaintenanceError> {
    start_insert(tenant.key, row, relid)?.to_query().execute(connection).await?;
    Ok(())
}

/// Record the run of `row` reaching `phase`.
pub(crate) async fn record_phase(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
    phase: RepackPhase,
) -> Result<(), MaintenanceError> {
    let Some(code) = phase.code() else {
        return Ok(());
    };
    phase_update(tenant.key, row, code).to_query().execute(connection).await?;
    Ok(())
}

/// Delete the record of the run of `row`, its objects dropped.
pub(crate) async fn record_end(connection: &mut PgConnection, tenant: TenantKey, row: BranchTarget) -> Result<(), MaintenanceError> {
    end_delete(tenant, row).to_query().execute(connection).await?;
    Ok(())
}

/// Every recorded run of `tenant`.
pub(crate) async fn tenant_runs(connection: &mut PgConnection, tenant: TenantKey) -> Result<Vec<RepackRunRow>, MaintenanceError> {
    let statement = runs_statement()
        .and_where(Expr::col(PostgresqlMaintenanceRepackRuns::TenantKey).eq(tenant.0))
        .to_owned();
    read_runs(connection, &statement).await
}

/// The tenants with a recorded run, each once, in key order.
pub(crate) async fn tenants_with_runs(connection: &mut PgConnection) -> Result<Vec<TenantKey>, MaintenanceError> {
    let mut tenants = read_runs(connection, &runs_statement())
        .await?
        .into_iter()
        .map(|run| run.tenant)
        .collect::<Vec<_>>();
    tenants.dedup();
    Ok(tenants)
}

/// Every recorded run, in tenant, branch and target order.
fn runs_statement() -> SelectStatement {
    Query::select()
        .columns([
            PostgresqlMaintenanceRepackRuns::TenantKey,
            PostgresqlMaintenanceRepackRuns::SnapshotKey,
            PostgresqlMaintenanceRepackRuns::Target,
            PostgresqlMaintenanceRepackRuns::Relid,
            PostgresqlMaintenanceRepackRuns::Phase,
            PostgresqlMaintenanceRepackRuns::StartedAt,
        ])
        .from(PostgresqlMaintenanceRepackRuns::Table)
        .order_by(PostgresqlMaintenanceRepackRuns::TenantKey, Order::Asc)
        .order_by(PostgresqlMaintenanceRepackRuns::SnapshotKey, Order::Asc)
        .order_by(PostgresqlMaintenanceRepackRuns::Target, Order::Asc)
        .to_owned()
}

/// Run a run statement and decode each row.
async fn read_runs(connection: &mut PgConnection, statement: &SelectStatement) -> Result<Vec<RepackRunRow>, MaintenanceError> {
    statement.to_query().fetch_all(connection).await?.iter().map(run_of).collect()
}

/// One stored run.
fn run_of(row: &PgRow) -> Result<RepackRunRow, MaintenanceError> {
    let column = |column: PostgresqlMaintenanceRepackRuns| column.to_string();
    let branch = branch_of_stored_key(row.try_get(column(PostgresqlMaintenanceRepackRuns::SnapshotKey).as_str())?)?;
    let target = target_of(MaintenanceTargetCode::try_from(
        row.try_get::<i16, _>(column(PostgresqlMaintenanceRepackRuns::Target).as_str())?,
    )?);
    Ok(RepackRunRow {
        tenant: TenantKey(row.try_get(column(PostgresqlMaintenanceRepackRuns::TenantKey).as_str())?),
        row: BranchTarget::new(branch, target)?,
        relid: object_id(
            row.try_get(column(PostgresqlMaintenanceRepackRuns::Relid).as_str())?,
            CatalogCounter::RelationOid,
        )?,
        phase: RepackPhase::of_code(MaintenanceRepackPhaseCode::try_from(
            row.try_get::<i16, _>(column(PostgresqlMaintenanceRepackRuns::Phase).as_str())?,
        )?),
        started_at: row.try_get(column(PostgresqlMaintenanceRepackRuns::StartedAt).as_str())?,
    })
}

/// The row of a run reaching its setup, replacing the row of an earlier run
/// of the same target that recovery has not deleted yet.
fn start_insert(tenant: TenantKey, row: BranchTarget, relid: u32) -> Result<InsertStatement, SeaQueryError> {
    let columns = [
        PostgresqlMaintenanceRepackRuns::TenantKey,
        PostgresqlMaintenanceRepackRuns::SnapshotKey,
        PostgresqlMaintenanceRepackRuns::Target,
    ];
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceRepackRuns::Table)
        .columns([
            PostgresqlMaintenanceRepackRuns::TenantKey,
            PostgresqlMaintenanceRepackRuns::SnapshotKey,
            PostgresqlMaintenanceRepackRuns::Target,
            PostgresqlMaintenanceRepackRuns::Relid,
            PostgresqlMaintenanceRepackRuns::Phase,
            PostgresqlMaintenanceRepackRuns::StartedAt,
            PostgresqlMaintenanceRepackRuns::PhaseChangedAt,
        ])
        .values([
            tenant.0.into(),
            stored_snapshot_key(row.branch()).into(),
            stored_target(row.target()).code().into(),
            i64::from(relid).into(),
            MaintenanceRepackPhaseCode::Setup.code().into(),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
        ])?
        .on_conflict(
            OnConflict::columns(columns)
                .update_columns([
                    PostgresqlMaintenanceRepackRuns::Relid,
                    PostgresqlMaintenanceRepackRuns::Phase,
                    PostgresqlMaintenanceRepackRuns::StartedAt,
                    PostgresqlMaintenanceRepackRuns::PhaseChangedAt,
                ])
                .to_owned(),
        );
    Ok(statement)
}

/// The update moving a run on to the phase `code`.
fn phase_update(tenant: TenantKey, row: BranchTarget, code: MaintenanceRepackPhaseCode) -> UpdateStatement {
    Query::update()
        .table(PostgresqlMaintenanceRepackRuns::Table)
        .value(PostgresqlMaintenanceRepackRuns::Phase, code.code())
        .value(PostgresqlMaintenanceRepackRuns::PhaseChangedAt, Expr::current_timestamp())
        .and_where(Expr::col(PostgresqlMaintenanceRepackRuns::TenantKey).eq(tenant.0))
        .and_where(Expr::col(PostgresqlMaintenanceRepackRuns::SnapshotKey).eq(stored_snapshot_key(row.branch())))
        .and_where(Expr::col(PostgresqlMaintenanceRepackRuns::Target).eq(stored_target(row.target()).code()))
        .to_owned()
}

/// The deletion of a run's row.
fn end_delete(tenant: TenantKey, row: BranchTarget) -> DeleteStatement {
    Query::delete()
        .from_table(PostgresqlMaintenanceRepackRuns::Table)
        .and_where(Expr::col(PostgresqlMaintenanceRepackRuns::TenantKey).eq(tenant.0))
        .and_where(Expr::col(PostgresqlMaintenanceRepackRuns::SnapshotKey).eq(stored_snapshot_key(row.branch())))
        .and_where(Expr::col(PostgresqlMaintenanceRepackRuns::Target).eq(stored_target(row.target()).code()))
        .to_owned()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::{Branch, BranchTarget},
        candidate::{MaintenanceTarget, heap_target::HeapTarget},
        state::{
            TenantKey,
            repack_run::{end_delete, phase_update, start_insert},
        },
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_codes::maintenance_repack_phase::MaintenanceRepackPhaseCode;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, error::Error as SeaQueryError};

    fn row() -> BranchTarget {
        let branch = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        BranchTarget::new(branch, MaintenanceTarget::Repack(HeapTarget::NotificationDeliveries)).expect("a branch heap")
    }

    #[test]
    fn a_run_starts_in_its_setup_and_replaces_a_row_recovery_has_not_deleted() -> Result<(), SeaQueryError> {
        let sql = start_insert(TenantKey(3), row(), 16_384)?.to_string(PostgresQueryBuilder);
        assert!(sql.contains("VALUES (3, 7, 50, 16384, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"), "{sql}");
        assert!(
            sql.contains(r#"ON CONFLICT ("tenant_key", "snapshot_key", "target") DO UPDATE SET "relid" = "excluded"."relid""#),
            "{sql}"
        );
        Ok(())
    }

    #[test]
    fn a_phase_and_the_end_address_the_runs_row() {
        assert_eq!(
            phase_update(TenantKey(3), row(), MaintenanceRepackPhaseCode::Replay).to_string(PostgresQueryBuilder),
            r#"UPDATE "postgresql_maintenance_repack_runs" SET "phase" = 4, "phase_changed_at" = CURRENT_TIMESTAMP WHERE "tenant_key" = 3 AND "snapshot_key" = 7 AND "target" = 50"#
        );
        assert_eq!(
            end_delete(TenantKey(3), row()).to_string(PostgresQueryBuilder),
            r#"DELETE FROM "postgresql_maintenance_repack_runs" WHERE "tenant_key" = 3 AND "snapshot_key" = 7 AND "target" = 50"#
        );
    }
}
