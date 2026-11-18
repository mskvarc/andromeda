//! The tenant template a registry prepares when it starts, on a fresh platform
//! database and on one v1.0.0 left alike: built under the name its build
//! derives and left refusing connections, rebuilt when unfinished, kept when it
//! is behind the migrations, every other template of the prefix dropped, the
//! tenants v1.0.0 established left as they are, and a template gone missing
//! leaving new tenants created empty.

use crate::fixtures::{
    database_catalog::{database_oid, first_migration_applied_at, template_flags},
    ledgers::{last_tenant_migration, public_applied_through},
    registry::{ACTIVE, drop_registry_template, fresh_prefix, insert_v1_0_0_tenant, open_registry, open_tenant_pool},
    test_error::UpgradeTestError,
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_context_store::{
    tenant_database::{DatabaseSource, create_database_if_missing},
    tenant_migration::{TenantSchemaState, tenant_schema_state},
    tenant_template::current_template_name,
};
use andromeda_postgresql_seaquery::{
    command::Command,
    database_definition::DatabaseDefinition,
    statement::Statement,
    timescale_scheduler::stop_background_workers,
};
use andromeda_tenancy::database::TenantDatabase;
use andromeda_upgrade_tests::{release_schema::V1_0_0_LAST_MIGRATION, upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::{assert_eq, assert_ne};
use sea_query::Alias;
use sqlx::PgPool;

/// Marks `database` a template that refuses connections, as a finished build
/// leaves it.
async fn mark_template(pool: &PgPool, database: &TenantDatabaseName) -> Result<(), UpgradeTestError> {
    DatabaseDefinition::mark_template(Alias::new(database.as_str()))
        .to_query()
        .execute(pool)
        .await?;
    Ok(())
}

/// Starts a registry of a fresh prefix on `platform`, checks it built the
/// template its build names, left it refusing connections and copies a new
/// tenant's database from it, then drops what it made beside `platform`.
async fn assert_started_registry_builds_and_copies_its_template(server: &UpgradeServer, platform: &UpgradeDatabase) -> Result<(), UpgradeTestError> {
    let prefix = fresh_prefix()?;

    let registry = open_registry(platform, &prefix).await?;

    let template = current_template_name(server.url(), &prefix).await?;
    assert!(template.as_str().starts_with(&format!("{prefix}-template-")), "{template}");
    assert_eq!(template_flags(platform.pool(), &template).await?, Some((true, false)));
    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;
    let pool = open_tenant_pool(server, &acme).await?;
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);

    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    drop_registry_template(server, &prefix).await
}

#[tokio::test]
async fn a_registry_started_on_a_fresh_platform_database_builds_the_template_and_copies_new_tenants_from_it() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;

    assert_started_registry_builds_and_copies_its_template(&server, &platform).await?;

    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_registry_started_on_a_v1_0_0_platform_database_builds_the_template_and_leaves_v1_0_0_tenants_alone() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    platform.migrate_platform_to_v1_0_0().await?;
    let established = UpgradeDatabase::create(&server).await?;
    established.migrate_to_v1_0_0().await?;
    insert_v1_0_0_tenant(platform.pool(), "initech", established.database_name().as_str(), ACTIVE).await?;
    let established_oid = database_oid(platform.pool(), established.database_name()).await?;

    assert_started_registry_builds_and_copies_its_template(&server, &platform).await?;

    assert_eq!(database_oid(platform.pool(), established.database_name()).await?, established_oid);
    assert_eq!(public_applied_through(established.pool()).await?, Some(V1_0_0_LAST_MIGRATION));
    established.drop_database().await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_template_of_another_build_is_dropped_and_an_unfinished_one_rebuilt_when_the_registry_starts() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let superseded = TenantDatabaseName::from(format!("{prefix}-template-00000000"));
    create_database_if_missing(server.url(), &superseded, DatabaseSource::ServerDefault).await?;
    mark_template(platform.pool(), &superseded).await?;
    let expected = current_template_name(server.url(), &prefix).await?;
    create_database_if_missing(server.url(), &expected, DatabaseSource::ServerDefault).await?;
    let unfinished_oid = database_oid(platform.pool(), &expected).await?;

    let registry = open_registry(&platform, &prefix).await?;

    assert_eq!(template_flags(platform.pool(), &superseded).await?, None);
    assert_eq!(template_flags(platform.pool(), &expected).await?, Some((true, false)));
    assert_ne!(database_oid(platform.pool(), &expected).await?, unfinished_oid);

    registry.main_pool().close().await;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_template_behind_this_builds_migrations_is_kept_and_its_copies_are_brought_forward() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let stale = UpgradeDatabase::create_named(&server, current_template_name(server.url(), &prefix).await?).await?;
    stale.migrate_to_v1_0_0().await?;
    stop_background_workers().to_query().execute(stale.pool()).await?;
    let template_first_migration = first_migration_applied_at(stale.pool()).await?;
    let template = stale.close().await;
    mark_template(platform.pool(), &template).await?;
    let template_oid = database_oid(platform.pool(), &template).await?;

    let registry = open_registry(&platform, &prefix).await?;
    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;

    assert_eq!(database_oid(platform.pool(), &template).await?, template_oid);
    let pool = open_tenant_pool(&server, &acme).await?;
    assert_eq!(first_migration_applied_at(&pool).await?, template_first_migration);
    assert_eq!(public_applied_through(&pool).await?, last_tenant_migration());
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);

    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_template_gone_missing_leaves_new_tenant_databases_created_empty_and_migrated() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let registry = open_registry(&platform, &prefix).await?;
    drop_registry_template(&server, &prefix).await?;

    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;

    let pool = open_tenant_pool(&server, &acme).await?;
    assert_eq!(public_applied_through(&pool).await?, last_tenant_migration());
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);

    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    platform.drop_database().await?;
    Ok(())
}
