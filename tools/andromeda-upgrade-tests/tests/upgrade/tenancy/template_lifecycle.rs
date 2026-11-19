//! The tenant template a registry prepares when it starts, on a fresh platform
//! database and on one v1.0.0 left alike: built as `template_<prefix>` and left
//! locked with no session on it, kept in place across starts, brought forward
//! in place when it is behind the migrations or was left unlocked, built again
//! when this build refuses its schema, the tenants v1.0.0 established left as
//! they are, and a template gone missing leaving new tenants created empty.

use crate::fixtures::{
    database_catalog::{database_oid, first_migration_applied_at, sessions_on, template_flags},
    ledgers::{last_tenant_migration, public_applied_through},
    registry::{ACTIVE, drop_registry_template, fresh_prefix, insert_v1_0_0_tenant, open_registry, open_tenant_pool},
    statements::name,
    test_error::UpgradeTestError,
};
use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_context_store::{
    tenant_database::{DatabaseSource, create_database_if_missing},
    tenant_migration::{TenantSchemaState, tenant_schema_state},
    tenant_template::template_name::template_database_name,
};
use andromeda_postgresql_seaquery::{
    command::Command,
    database_definition::DatabaseDefinition,
    statement::Statement,
    timescale_scheduler::stop_background_workers,
};
use andromeda_tenancy::database::TenantDatabase;
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::{assert_eq, assert_ne};
use sea_query::{Alias, Expr, ExprTrait, Query};
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// How long a closed session may take to leave the server's activity view.
const SESSION_DRAIN: Duration = Duration::from_secs(10);

/// Locks `database` as a finished start leaves the template, after stopping
/// the scheduler of the database `inside` reaches.
async fn lock_template(platform: &PgPool, inside: &PgPool, database: &TenantDatabaseName) -> Result<(), UpgradeTestError> {
    stop_background_workers().to_query().execute(inside).await?;
    DatabaseDefinition::lock_template(Alias::new(database.as_str()))
        .to_query()
        .execute(platform)
        .await?;
    Ok(())
}

/// How many sessions hold `database` once closed ones have had
/// [`SESSION_DRAIN`] to leave.
async fn sessions_after_drain(pool: &PgPool, database: &TenantDatabaseName) -> Result<i64, UpgradeTestError> {
    let deadline = Instant::now() + SESSION_DRAIN;
    loop {
        let sessions = sessions_on(pool, database).await?;
        if sessions == 0 || Instant::now() >= deadline {
            return Ok(sessions);
        }
        sleep(Duration::from_millis(100)).await;
    }
}

/// Provisions `acme` through a registry of `prefix` on `platform`, checks its
/// database is at this build's schema, and drops it.
async fn assert_new_tenant_is_current(server: &UpgradeServer, platform: &UpgradeDatabase, prefix: &DbNamePrefix) -> Result<(), UpgradeTestError> {
    let registry = open_registry(platform, prefix).await?;
    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;
    let pool = open_tenant_pool(server, &acme).await?;
    assert_eq!(public_applied_through(&pool).await?, last_tenant_migration());
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);
    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    Ok(())
}

/// Starts a registry of a fresh prefix on `platform`, checks it built
/// `template_<prefix>`, left it locked with no session on it and copies a new
/// tenant's database from it, then drops what it made beside `platform`.
async fn assert_started_registry_builds_and_copies_its_template(server: &UpgradeServer, platform: &UpgradeDatabase) -> Result<(), UpgradeTestError> {
    let prefix = fresh_prefix()?;
    let template = template_database_name(&prefix);
    assert_eq!(template.as_str(), format!("template_{prefix}"));

    let registry = open_registry(platform, &prefix).await?;

    assert_eq!(template_flags(platform.pool(), &template).await?, Some((true, false)));
    assert_eq!(sessions_after_drain(platform.pool(), &template).await?, 0);
    registry.main_pool().close().await;
    assert_new_tenant_is_current(server, platform, &prefix).await?;
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
    let established_ledger = public_applied_through(established.pool()).await?;

    assert_started_registry_builds_and_copies_its_template(&server, &platform).await?;

    assert_eq!(database_oid(platform.pool(), established.database_name()).await?, established_oid);
    assert_eq!(public_applied_through(established.pool()).await?, established_ledger);
    established.drop_database().await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_registry_started_again_keeps_its_template_in_place_and_locked() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let template = template_database_name(&prefix);
    open_registry(&platform, &prefix).await?.main_pool().close().await;
    let built_oid = database_oid(platform.pool(), &template).await?;

    open_registry(&platform, &prefix).await?.main_pool().close().await;

    assert_eq!(database_oid(platform.pool(), &template).await?, built_oid);
    assert_eq!(template_flags(platform.pool(), &template).await?, Some((true, false)));
    assert_eq!(sessions_after_drain(platform.pool(), &template).await?, 0);
    assert_new_tenant_is_current(&server, &platform, &prefix).await?;

    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_template_behind_this_builds_migrations_is_brought_forward_in_place() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let stale = UpgradeDatabase::create_named(&server, template_database_name(&prefix)).await?;
    stale.migrate_to_v1_0_0().await?;
    let template_first_migration = first_migration_applied_at(stale.pool()).await?;
    lock_template(platform.pool(), stale.pool(), stale.database_name()).await?;
    let template = stale.close().await;
    let template_oid = database_oid(platform.pool(), &template).await?;

    let registry = open_registry(&platform, &prefix).await?;
    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;

    assert_eq!(database_oid(platform.pool(), &template).await?, template_oid);
    assert_eq!(template_flags(platform.pool(), &template).await?, Some((true, false)));
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
async fn a_template_a_start_left_unlocked_and_unmigrated_is_brought_forward_and_locked_in_place() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let template = template_database_name(&prefix);
    create_database_if_missing(server.url(), &template, DatabaseSource::ServerDefault).await?;
    let unfinished_oid = database_oid(platform.pool(), &template).await?;

    open_registry(&platform, &prefix).await?.main_pool().close().await;

    assert_eq!(database_oid(platform.pool(), &template).await?, unfinished_oid);
    assert_eq!(template_flags(platform.pool(), &template).await?, Some((true, false)));
    assert_new_tenant_is_current(&server, &platform, &prefix).await?;

    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_template_whose_stamp_is_ahead_of_this_build_is_built_again() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let mut ahead = UpgradeDatabase::create_named(&server, template_database_name(&prefix)).await?;
    ahead.migrate_to_current().await?;
    Query::update()
        .table(name("schema_metadata"))
        .value(name("schema_version"), Expr::col(name("schema_version")).add(1))
        .to_owned()
        .to_query()
        .execute(ahead.pool())
        .await?;
    lock_template(platform.pool(), ahead.pool(), ahead.database_name()).await?;
    let template = ahead.close().await;
    let ahead_oid = database_oid(platform.pool(), &template).await?;

    open_registry(&platform, &prefix).await?.main_pool().close().await;

    assert_ne!(database_oid(platform.pool(), &template).await?, ahead_oid);
    assert_eq!(template_flags(platform.pool(), &template).await?, Some((true, false)));
    assert_new_tenant_is_current(&server, &platform, &prefix).await?;

    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_template_stamped_with_another_migration_set_at_this_version_is_built_again() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let mut foreign = UpgradeDatabase::create_named(&server, template_database_name(&prefix)).await?;
    foreign.migrate_to_current().await?;
    Query::update()
        .table(name("schema_metadata"))
        .value(name("migrations_sha256"), Expr::val("0".repeat(64)))
        .to_owned()
        .to_query()
        .execute(foreign.pool())
        .await?;
    lock_template(platform.pool(), foreign.pool(), foreign.database_name()).await?;
    let template = foreign.close().await;
    let foreign_oid = database_oid(platform.pool(), &template).await?;

    open_registry(&platform, &prefix).await?.main_pool().close().await;

    assert_ne!(database_oid(platform.pool(), &template).await?, foreign_oid);
    assert_eq!(template_flags(platform.pool(), &template).await?, Some((true, false)));
    assert_new_tenant_is_current(&server, &platform, &prefix).await?;

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

#[tokio::test]
async fn a_template_a_session_holds_leaves_the_new_tenant_created_empty_and_migrated() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let registry = open_registry(&platform, &prefix).await?;
    let template = template_database_name(&prefix);
    DatabaseDefinition::unlock_template(Alias::new(template.as_str()))
        .to_query()
        .execute(platform.pool())
        .await?;
    // The one-connection pool keeps its session open on the template while
    // the tenant is provisioned.
    let holder = open_tenant_pool(&server, &template).await?;
    let template_first_migration = first_migration_applied_at(&holder).await?;

    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;

    let pool = open_tenant_pool(&server, &acme).await?;
    assert_eq!(public_applied_through(&pool).await?, last_tenant_migration());
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);
    assert_ne!(first_migration_applied_at(&pool).await?, template_first_migration);

    pool.close().await;
    holder.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_template_a_session_holds_when_it_must_be_built_again_is_left_and_new_tenants_are_created_empty() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let mut ahead = UpgradeDatabase::create_named(&server, template_database_name(&prefix)).await?;
    ahead.migrate_to_current().await?;
    Query::update()
        .table(name("schema_metadata"))
        .value(name("schema_version"), Expr::col(name("schema_version")).add(1))
        .to_owned()
        .to_query()
        .execute(ahead.pool())
        .await?;
    let held_oid = database_oid(platform.pool(), ahead.database_name()).await?;

    let registry = open_registry(&platform, &prefix).await?;
    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;

    assert_eq!(database_oid(platform.pool(), ahead.database_name()).await?, held_oid);
    let pool = open_tenant_pool(&server, &acme).await?;
    assert_eq!(public_applied_through(&pool).await?, last_tenant_migration());
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);

    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    ahead.close().await;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}
