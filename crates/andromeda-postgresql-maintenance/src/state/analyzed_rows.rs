//! The analyzed row count stored on each heap and density-measured GIN target
//! row.

use crate::{
    analyzed_rows::{AnalyzedRows, RowCountReading},
    branch::{Branch, BranchTarget},
    candidate::MaintenanceTarget,
    catalog_counter::{CatalogCounter, non_negative, persisted_count},
    error::MaintenanceError,
    evidence::target_code::{stored_target, target_of},
    state::{
        RegisteredTenant,
        branch_rows::{branch_rows, stored_snapshot_key},
        target_record::TARGET_GRAIN,
    },
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, InsertStatement, OnConflict, Query};
use sqlx::{PgConnection, Row};

/// The analyzed rows stored on the target rows of one branch.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StoredAnalyzedRows(Vec<(MaintenanceTarget, AnalyzedRows)>);

impl StoredAnalyzedRows {
    /// The rows stored for `target`, `None` before its first reading after an
    /// analyze.
    pub(crate) fn of(&self, target: MaintenanceTarget) -> Option<AnalyzedRows> {
        self.0.iter().find(|(stored, _)| *stored == target).map(|(_, rows)| *rows)
    }
}

/// Read the analyzed rows stored on every target row of `branch`.
pub(crate) async fn stored_analyzed_rows(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
) -> Result<StoredAnalyzedRows, MaintenanceError> {
    let statement = Query::select()
        .columns([
            PostgresqlMaintenanceTargets::Target,
            PostgresqlMaintenanceTargets::AnalyzedRows,
            PostgresqlMaintenanceTargets::RowsAnalyzedAt,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(branch_rows(branch))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::AnalyzedRows).is_not_null())
        .to_owned();
    let rows = statement.to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let code = MaintenanceTargetCode::try_from(row.try_get::<i16, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?)?;
            let analyzed_at: DateTime<Utc> = row.try_get(PostgresqlMaintenanceTargets::RowsAnalyzedAt.to_string().as_str())?;
            let rows = non_negative(
                row.try_get(PostgresqlMaintenanceTargets::AnalyzedRows.to_string().as_str())?,
                CatalogCounter::AnalyzedRows,
            )?;
            Ok((target_of(code), AnalyzedRows { rows, analyzed_at }))
        })
        .collect::<Result<_, MaintenanceError>>()
        .map(StoredAnalyzedRows)
}

/// The analyzed rows to judge `row` by: `reading`'s when it replaces `stored`,
/// which are then recorded in its place, else `stored`. `None` until the
/// target is read after its table's first analyze.
pub(crate) async fn settled_rows(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
    reading: RowCountReading,
    stored: Option<AnalyzedRows>,
) -> Result<Option<AnalyzedRows>, MaintenanceError> {
    let Some(replacement) = reading.replacement(stored) else {
        return Ok(stored);
    };
    analyzed_rows_upsert(tenant, row, replacement)?.to_query().execute(connection).await?;
    Ok(Some(replacement))
}

/// The analyzed rows upsert, creating the target row on its first reading and
/// leaving every other column of an existing row as it was.
fn analyzed_rows_upsert(tenant: &RegisteredTenant, row: BranchTarget, rows: AnalyzedRows) -> Result<InsertStatement, MaintenanceError> {
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTargets::Table)
        .columns(
            TARGET_GRAIN
                .into_iter()
                .chain([PostgresqlMaintenanceTargets::AnalyzedRows, PostgresqlMaintenanceTargets::RowsAnalyzedAt]),
        )
        .values([
            tenant.key.0.into(),
            stored_snapshot_key(row.branch()).into(),
            stored_target(row.target()).code().into(),
            persisted_count(rows.rows, CatalogCounter::AnalyzedRows)?.into(),
            rows.analyzed_at.into(),
        ])?
        .on_conflict(
            OnConflict::columns(TARGET_GRAIN)
                .update_columns([PostgresqlMaintenanceTargets::AnalyzedRows, PostgresqlMaintenanceTargets::RowsAnalyzedAt])
                .to_owned(),
        );
    Ok(statement)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        analyzed_rows::AnalyzedRows,
        branch::{Branch, BranchTarget},
        candidate::{MaintenanceTarget, heap_target::HeapTarget, index_target::IndexTarget},
        error::MaintenanceError,
        state::{
            RegisteredTenant,
            TenantKey,
            analyzed_rows::{StoredAnalyzedRows, analyzed_rows_upsert},
        },
    };
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn tenant() -> RegisteredTenant {
        RegisteredTenant {
            key: TenantKey(7),
            name: None,
            database: TenantDatabaseName::from("andromeda".to_owned()),
        }
    }

    fn rows(rows: u64) -> AnalyzedRows {
        AnalyzedRows {
            rows,
            analyzed_at: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    #[test]
    fn analyzed_rows_are_upserted_with_their_instant_without_touching_the_rest_of_the_row() -> Result<(), MaintenanceError> {
        let row = BranchTarget::new(Branch::Live, MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys)).expect("the live branch holds every target");
        let sql = analyzed_rows_upsert(&tenant(), row, rows(850_000))?.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"("tenant_key", "snapshot_key", "target", "analyzed_rows", "rows_analyzed_at")"#),
            "{sql}"
        );
        assert!(sql.contains("VALUES (7, 0, 10, 850000, '1970-01-01 00:00:00.000000 +00:00')"), "{sql}");
        assert!(
            sql.ends_with(
                r#"ON CONFLICT ("tenant_key", "snapshot_key", "target") DO UPDATE SET "analyzed_rows" = "excluded"."analyzed_rows", "rows_analyzed_at" = "excluded"."rows_analyzed_at""#
            ),
            "{sql}"
        );
        Ok(())
    }

    #[test]
    fn stored_rows_are_found_by_their_own_target_only() {
        let heap = MaintenanceTarget::Heap(HeapTarget::CurrentAttributes);
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys);
        let stored = StoredAnalyzedRows(vec![(heap, rows(3_000_000)), (gin, rows(850_000))]);
        assert_eq!(stored.of(heap), Some(rows(3_000_000)));
        assert_eq!(stored.of(gin), Some(rows(850_000)));
        assert_eq!(stored.of(MaintenanceTarget::Index(IndexTarget::IdxCaValueObject)), None);
    }
}
