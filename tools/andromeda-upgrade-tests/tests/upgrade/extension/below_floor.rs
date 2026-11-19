//! A tenant database whose `TimescaleDB` stands below the schema contract's
//! floor while the server ships a version above it, as a database created on
//! an image older than the first stable one leaves it. The probe reads it as
//! needing migration, since the update lifts it past the floor, and the
//! migration run updates it and accepts it.

use crate::fixtures::{registry::database_defaults, test_error::UpgradeTestError};
use andromeda_postgresql_context_store::{
    schema_contract::server_requirements::REQUIRED_TIMESCALEDB_VERSION,
    tenant_migration::{TenantSchemaState, migrate_single_tenant_db, tenant_schema_state},
};
use andromeda_postgresql_seaquery::{command::Command, introspection::extension_version, statement::Statement, test_extension::ExtensionCreationAtVersion};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::assert_eq;
use sea_query::{Alias, Query};
use std::time::Duration;

/// A `TimescaleDB` release below the floor that the image still ships.
const BELOW_FLOOR: &str = "2.30.2";

#[tokio::test]
async fn a_timescaledb_below_the_floor_with_an_update_shipped_is_migrated_rather_than_refused() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let database = UpgradeDatabase::create(&server).await?;
    ExtensionCreationAtVersion::new(Alias::new("timescaledb"), BELOW_FLOOR)
        .to_query()
        .execute(database.pool())
        .await?;
    database.migrate_to_v1_0_0().await?;
    let url = server.url().with_database_name(database.database_name());

    assert_eq!(tenant_schema_state(database.pool()).await?, TenantSchemaState::NeedsMigration);
    migrate_single_tenant_db(&url, &database_defaults(), Duration::from_secs(30)).await?;

    let installed = Query::select()
        .expr(extension_version("timescaledb"))
        .to_owned()
        .to_query_scalar::<Option<String>>()
        .fetch_one(database.pool())
        .await?;
    assert!(
        installed.is_some_and(|version| version != BELOW_FLOOR),
        "updated past {BELOW_FLOOR}, the floor being {REQUIRED_TIMESCALEDB_VERSION}"
    );
    assert_eq!(tenant_schema_state(database.pool()).await?, TenantSchemaState::Current);
    database.drop_database().await?;
    Ok(())
}
