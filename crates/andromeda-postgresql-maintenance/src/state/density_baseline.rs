//! The recorded density baseline of each density-measured GIN.

use crate::{
    candidate::{DensityColumn, IndexTarget, MaintenanceTarget},
    catalog_counter::{CatalogCounter, persisted_count},
    error::MaintenanceError,
    evidence::target_code::{stored_target, target_of},
    gin_density::Density,
    state::RegisteredTenant,
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use sea_query::{Expr, ExprTrait, Iden, InsertStatement, OnConflict, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};
use std::num::NonZeroU64;

/// One density-measured GIN's recorded baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DensityBaseline {
    /// The GIN.
    pub(crate) target: IndexTarget,
    /// Its density when first observed or last rebuilt.
    pub(crate) density: Density,
}

/// Read every density-measured GIN's recorded baseline for one tenant; a GIN
/// without one is absent.
pub(crate) async fn density_baselines(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<Vec<DensityBaseline>, MaintenanceError> {
    let codes = DensityColumn::ALL.map(|column| stored_target(MaintenanceTarget::Index(column.gin())).code());
    let statement = Query::select()
        .columns([PostgresqlMaintenanceTargets::Target, PostgresqlMaintenanceTargets::BaselineDensity])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).is_in(codes))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::BaselineDensity).is_not_null())
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let code = MaintenanceTargetCode::try_from(row.try_get::<i16, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?)?;
            let value = row.try_get::<i64, _>(PostgresqlMaintenanceTargets::BaselineDensity.to_string().as_str())?;
            let invalid = || MaintenanceError::InvalidDensityBaseline { target: code, value };
            let MaintenanceTarget::Index(target) = target_of(code) else {
                return Err(invalid());
            };
            let millibytes = u64::try_from(value).ok().and_then(NonZeroU64::new).ok_or_else(invalid)?;
            target.density_column().ok_or_else(invalid)?;
            Ok(DensityBaseline {
                target,
                density: Density::from_millibytes(millibytes),
            })
        })
        .collect()
}

/// Record `baseline` as the GIN's baseline, creating its target row on its
/// first observation. `None` clears it, so the next observation records one.
pub(crate) async fn record_density_baseline(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: IndexTarget,
    baseline: Option<Density>,
) -> Result<(), MaintenanceError> {
    let (sql, values) = density_baseline_upsert(tenant, target, baseline)?.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

/// The baseline upsert, leaving every other column of an existing row as it
/// was.
fn density_baseline_upsert(tenant: &RegisteredTenant, target: IndexTarget, baseline: Option<Density>) -> Result<InsertStatement, MaintenanceError> {
    let stored = baseline
        .map(|density| persisted_count(density.millibytes_per_row(), CatalogCounter::BaselineDensity))
        .transpose()?;
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTargets::Table)
        .columns([
            PostgresqlMaintenanceTargets::TenantKey,
            PostgresqlMaintenanceTargets::Target,
            PostgresqlMaintenanceTargets::BaselineDensity,
        ])
        .values([
            tenant.key.0.into(),
            stored_target(MaintenanceTarget::Index(target)).code().into(),
            stored.into(),
        ])?
        .on_conflict(
            OnConflict::columns([PostgresqlMaintenanceTargets::TenantKey, PostgresqlMaintenanceTargets::Target])
                .update_column(PostgresqlMaintenanceTargets::BaselineDensity)
                .to_owned(),
        );
    Ok(statement)
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::IndexTarget,
        error::MaintenanceError,
        gin_density::Density,
        state::{RegisteredTenant, TenantKey, density_baseline::density_baseline_upsert},
    };
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use sea_query::PostgresQueryBuilder;
    use std::num::NonZeroU64;

    fn tenant() -> RegisteredTenant {
        RegisteredTenant {
            key: TenantKey(7),
            name: None,
            database: TenantDatabaseName::from("andromeda".to_owned()),
        }
    }

    #[test]
    fn a_baseline_is_upserted_without_touching_the_rest_of_the_row() -> Result<(), MaintenanceError> {
        let density = NonZeroU64::new(2_500).map(Density::from_millibytes);
        let sql = density_baseline_upsert(&tenant(), IndexTarget::IdxCaValueObject, density)?.to_string(PostgresQueryBuilder);
        assert!(sql.contains("VALUES (7, 14, 2500)"), "{sql}");
        assert!(
            sql.ends_with(r#"ON CONFLICT ("tenant_key", "target") DO UPDATE SET "baseline_density" = "excluded"."baseline_density""#),
            "{sql}"
        );
        Ok(())
    }

    #[test]
    fn a_cleared_baseline_is_stored_as_null() -> Result<(), MaintenanceError> {
        let sql = density_baseline_upsert(&tenant(), IndexTarget::IdxCaSubEqKeys, None)?.to_string(PostgresQueryBuilder);
        assert!(sql.contains("VALUES (7, 10, NULL)"), "{sql}");
        Ok(())
    }
}
