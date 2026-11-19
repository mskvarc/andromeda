//! A tenant database stamped with a schema version ahead of this build, as a
//! newer broker leaves it, is refused by the probe and by the migration run,
//! and neither changes its stamp or its ledger.

use crate::fixtures::{statements::name, test_error::UpgradeTestError};
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    migration_set::tenant::TENANT_LEDGER,
    schema_contract::error::SchemaContractError,
    tenant_migration::{migrate_tenant_db, tenant_schema_state},
};
use andromeda_postgresql_seaquery::{aggregate::count_all, cast::cast, pg_type::PostgresType, statement::Statement};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query};
use sqlx::PgPool;

/// The stamp's version and instant, and the ledger's row count, as text.
async fn stamp_and_ledger(pool: &PgPool) -> Result<(String, String, i64), UpgradeTestError> {
    let (version, applied_at) = Query::select()
        .expr(cast(Expr::col(name("schema_version")), PostgresType::Text))
        .expr(cast(Expr::col(name("applied_at")), PostgresType::Text))
        .from(name("schema_metadata"))
        .to_owned()
        .to_query_as::<(String, String)>()
        .fetch_one(pool)
        .await?;
    let ledger_rows = Query::select()
        .expr(count_all())
        .from(name(TENANT_LEDGER))
        .to_owned()
        .to_query_scalar::<i64>()
        .fetch_one(pool)
        .await?;
    Ok((version, applied_at, ledger_rows))
}

/// Whether `outcome` refuses a stamp ahead of this build's contract.
fn is_ahead<T>(outcome: &Result<T, ContextStoreError>) -> bool {
    matches!(outcome, Err(ContextStoreError::SchemaContract(SchemaContractError::SchemaVersionMismatch { expected, found })) if found > expected)
}

#[tokio::test]
async fn a_tenant_stamped_ahead_of_this_build_is_refused_and_left_as_it_was() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let mut database = UpgradeDatabase::create(&server).await?;
    database.migrate_to_current().await?;
    Query::update()
        .table(name("schema_metadata"))
        .value(name("schema_version"), Expr::col(name("schema_version")).add(1))
        .to_owned()
        .to_query()
        .execute(database.pool())
        .await?;
    let before = stamp_and_ledger(database.pool()).await?;

    let probed = tenant_schema_state(database.pool()).await;
    let migrated = migrate_tenant_db(database.pool()).await;

    assert!(is_ahead(&probed), "the probe refuses the stamp: {probed:?}");
    assert!(is_ahead(&migrated), "the migration run refuses the stamp: {migrated:?}");
    assert_eq!(stamp_and_ledger(database.pool()).await?, before);
    database.drop_database().await?;
    Ok(())
}
