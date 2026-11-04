//! The keys per row and the bytes per key stored on each density-measured
//! GIN's target row.

use crate::{
    branch::Branch,
    candidate::{
        MaintenanceTarget,
        index_target::{DensityColumn, IndexTarget},
    },
    catalog_counter::{CatalogCounter, persisted_count},
    error::MaintenanceError,
    evidence::target_code::{stored_target, target_of},
    gin_keys::{KeyBytes, KeysPerRow},
    state::{
        RegisteredTenant,
        branch_rows::{branch_rows, stored_snapshot_key},
        target_record::TARGET_GRAIN,
    },
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use andromeda_postgresql_seaquery::statement::Statement;
use sea_query::{Expr, ExprTrait, Iden, InsertStatement, OnConflict, Query, Value};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::num::NonZeroU64;

/// What one density-measured GIN's target row holds of its key layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StoredKeyLayout {
    /// The column the GIN covers.
    column: DensityColumn,
    /// The keys per row last sampled, `None` before the first sample.
    keys_per_row: Option<KeysPerRow>,
    /// The bytes per key last measured on the GIN, `None` before its first
    /// rebuild, or its first observation for a shape with no default.
    key_bytes: Option<KeyBytes>,
}

/// The key layouts stored on the density-measured GINs of one branch.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StoredKeyLayouts(Vec<StoredKeyLayout>);

impl StoredKeyLayouts {
    /// The stored layout of the GIN over `column`, if its row holds one.
    fn of(&self, column: DensityColumn) -> Option<StoredKeyLayout> {
        self.0.iter().find(|layout| layout.column == column).copied()
    }

    /// The keys per row stored for the GIN over `column`.
    pub(crate) fn keys_per_row(&self, column: DensityColumn) -> Option<KeysPerRow> {
        self.of(column).and_then(|layout| layout.keys_per_row)
    }

    /// The bytes per key stored for the GIN over `column`.
    pub(crate) fn key_bytes(&self, column: DensityColumn) -> Option<KeyBytes> {
        self.of(column).and_then(|layout| layout.key_bytes)
    }
}

/// Read the key layout stored on every density-measured GIN of `branch`.
pub(crate) async fn stored_key_layouts(connection: &mut PgConnection, tenant: &RegisteredTenant, branch: Branch) -> Result<StoredKeyLayouts, MaintenanceError> {
    let codes = DensityColumn::ALL.map(|column| stored_target(MaintenanceTarget::Index(column.gin())).code());
    let statement = Query::select()
        .columns([
            PostgresqlMaintenanceTargets::Target,
            PostgresqlMaintenanceTargets::KeysPerRow,
            PostgresqlMaintenanceTargets::KeyBytes,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(branch_rows(branch))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).is_in(codes))
        .to_owned();
    let rows = statement.to_query().fetch_all(connection).await?;
    rows.iter().map(stored_layout).collect::<Result<_, _>>().map(StoredKeyLayouts)
}

/// Decode one target row's key layout.
fn stored_layout(row: &PgRow) -> Result<StoredKeyLayout, MaintenanceError> {
    let code = MaintenanceTargetCode::try_from(row.try_get::<i16, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?)?;
    let column = match target_of(code) {
        MaintenanceTarget::Index(target) => target.density_column(),
        MaintenanceTarget::Heap(_) | MaintenanceTarget::Statistics(_) | MaintenanceTarget::Columnstore(_) | MaintenanceTarget::Rowstore(_) => None,
    }
    .ok_or(MaintenanceError::KeyLayoutTargetNotDensityMeasured(code))?;
    Ok(StoredKeyLayout {
        column,
        keys_per_row: stored_thousandths(row, PostgresqlMaintenanceTargets::KeysPerRow, code)?.map(KeysPerRow::from_thousandths),
        key_bytes: stored_thousandths(row, PostgresqlMaintenanceTargets::KeyBytes, code)?.map(KeyBytes::from_thousandths),
    })
}

/// One stored thousandths column of `row`, `None` while it is NULL.
fn stored_thousandths(row: &PgRow, column: PostgresqlMaintenanceTargets, target: MaintenanceTargetCode) -> Result<Option<NonZeroU64>, MaintenanceError> {
    row.try_get::<Option<i64>, _>(column.to_string().as_str())?
        .map(|value| {
            u64::try_from(value)
                .ok()
                .and_then(NonZeroU64::new)
                .ok_or(MaintenanceError::InvalidKeyLayout { target, value })
        })
        .transpose()
}

/// Record `keys_per_row` as the keys per row of the GIN over `column` in
/// `branch`.
pub(crate) async fn record_keys_per_row(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    column: DensityColumn,
    keys_per_row: KeysPerRow,
) -> Result<(), MaintenanceError> {
    let value = persisted_count(keys_per_row.thousandths(), CatalogCounter::KeysPerRow)?;
    layout_upsert(tenant, branch, column.gin(), PostgresqlMaintenanceTargets::KeysPerRow, value.into())?
        .to_query()
        .execute(connection)
        .await?;
    Ok(())
}

/// Record `key_bytes` as the bytes per key of the GIN over `column` in
/// `branch`. `None` clears them, so the GIN is judged at its shape's default,
/// or measured on its next observation when its shape has none.
pub(crate) async fn record_key_bytes(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    column: DensityColumn,
    key_bytes: Option<KeyBytes>,
) -> Result<(), MaintenanceError> {
    let value = key_bytes
        .map(|bytes| persisted_count(bytes.thousandths(), CatalogCounter::KeyBytes))
        .transpose()?;
    layout_upsert(tenant, branch, column.gin(), PostgresqlMaintenanceTargets::KeyBytes, value.into())?
        .to_query()
        .execute(connection)
        .await?;
    Ok(())
}

/// The upsert of one key layout column, creating the target row on its first
/// write and leaving every other column of an existing row as it was.
fn layout_upsert(
    tenant: &RegisteredTenant,
    branch: Branch,
    target: IndexTarget,
    column: PostgresqlMaintenanceTargets,
    value: Value,
) -> Result<InsertStatement, MaintenanceError> {
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTargets::Table)
        .columns(TARGET_GRAIN.into_iter().chain([column]))
        .values([
            tenant.key.0.into(),
            stored_snapshot_key(branch).into(),
            stored_target(MaintenanceTarget::Index(target)).code().into(),
            value.into(),
        ])?
        .on_conflict(OnConflict::columns(TARGET_GRAIN).update_column(column).to_owned());
    Ok(statement)
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::index_target::IndexTarget,
        error::MaintenanceError,
        state::{RegisteredTenant, TenantKey, gin_key_layout::layout_upsert},
    };
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
    use sea_query::{PostgresQueryBuilder, Value};

    fn tenant() -> RegisteredTenant {
        RegisteredTenant {
            key: TenantKey(7),
            name: None,
            database: TenantDatabaseName::from("andromeda".to_owned()),
        }
    }

    #[test]
    fn keys_per_row_are_upserted_without_touching_the_rest_of_the_row() -> Result<(), MaintenanceError> {
        let sql = layout_upsert(
            &tenant(),
            Branch::Live,
            IndexTarget::IdxEntitiesIdTrgm,
            PostgresqlMaintenanceTargets::KeysPerRow,
            124_198_i64.into(),
        )?
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"("tenant_key", "snapshot_key", "target", "keys_per_row")"#), "{sql}");
        assert!(sql.contains("VALUES (7, 0, 36, 124198)"), "{sql}");
        assert!(
            sql.ends_with(r#"ON CONFLICT ("tenant_key", "snapshot_key", "target") DO UPDATE SET "keys_per_row" = "excluded"."keys_per_row""#),
            "{sql}"
        );
        Ok(())
    }

    #[test]
    fn cleared_bytes_per_key_are_stored_as_null() -> Result<(), MaintenanceError> {
        let sql = layout_upsert(
            &tenant(),
            Branch::Live,
            IndexTarget::IdxCaSubEqKeys,
            PostgresqlMaintenanceTargets::KeyBytes,
            Value::BigInt(None),
        )?
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains("VALUES (7, 0, 10, NULL)"), "{sql}");
        assert!(
            sql.ends_with(r#"ON CONFLICT ("tenant_key", "snapshot_key", "target") DO UPDATE SET "key_bytes" = "excluded"."key_bytes""#),
            "{sql}"
        );
        Ok(())
    }
}
