//! A tenant database whose `TimescaleDB` stands at the schema contract's floor,
//! below the version the server ships, with the extension owned by a
//! superuser. A role that does not hold the owner's privileges reads the
//! schema as current, since no migration run it makes could update the
//! extension, and its extension reconciliation leaves the installed version in
//! place. The owner's reconciliation updates it and ends the idle sessions
//! that loaded the floor's library, so a pool that held one opens a new one
//! rather than meet `TimescaleDB`'s refusal of a session using a replaced
//! library.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_postgresql_context_store::{
    extension_update::update_extensions,
    schema_contract::server_requirements::REQUIRED_TIMESCALEDB_VERSION,
    tenant_migration::{TenantSchemaState, open_schema_change_pool, tenant_schema_state},
};
use andromeda_postgresql_seaquery::{
    command::Command,
    introspection::extension_version,
    statement::Statement,
    test_extension::ExtensionCreationAtVersion,
    test_role::{PublicTablesReadGrant, RoleCreation, RoleRemoval},
};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::{assert_eq, assert_ne};
use sea_query::{Alias, Query};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// How long a pool waits for a connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// The `TimescaleDB` version `pool`'s database holds.
async fn timescaledb_version(pool: &PgPool) -> Result<Option<String>, UpgradeTestError> {
    let select = Query::select().expr(extension_version("timescaledb")).to_owned();
    Ok(select.to_query_scalar::<Option<String>>().fetch_one(pool).await?)
}

#[tokio::test]
async fn a_timescaledb_update_only_its_owner_may_make_is_left_by_another_role_and_made_by_the_owner() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let mut database = UpgradeDatabase::create(&server).await?;
    let floor = REQUIRED_TIMESCALEDB_VERSION.to_string();
    ExtensionCreationAtVersion::new(Alias::new("timescaledb"), &floor)
        .to_query()
        .execute(database.pool())
        .await?;
    database.migrate_to_current().await?;
    assert_eq!(timescaledb_version(database.pool()).await?.as_deref(), Some(floor.as_str()));
    let reader = format!("reader_{}", Uuid::new_v4().simple());
    RoleCreation::new(Alias::new(&reader)).to_query().execute(database.pool()).await?;
    PublicTablesReadGrant::new(Alias::new(&reader)).to_query().execute(database.pool()).await?;
    let owner_url = server.url().with_database_name(database.database_name());
    let mut reader_url = owner_url.clone();
    reader_url.query_pairs_mut().append_pair("options", &format!("-c role={reader}"));
    let reader_pool = open_schema_change_pool(&reader_url, ACQUIRE_TIMEOUT).await?;

    assert_eq!(tenant_schema_state(&reader_pool).await?, TenantSchemaState::Current);
    update_extensions(&reader_url).await?;
    assert_eq!(timescaledb_version(&reader_pool).await?.as_deref(), Some(floor.as_str()));

    update_extensions(&owner_url).await?;

    assert_ne!(timescaledb_version(database.pool()).await?.as_deref(), Some(floor.as_str()));
    assert_eq!(tenant_schema_state(&reader_pool).await?, TenantSchemaState::Current);

    reader_pool.close().await;
    RoleRemoval::new(Alias::new(&reader)).to_query().execute(database.pool()).await?;
    database.drop_database().await?;
    Ok(())
}
