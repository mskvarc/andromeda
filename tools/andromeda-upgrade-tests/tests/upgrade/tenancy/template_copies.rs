//! A tenant database the registry provisions is a copy of the tenant
//! template, carries the whole tenant schema and a stamp instant of its own,
//! and records temporal history; a database an earlier provisioning left is
//! migrated where it stands instead.

use crate::fixtures::{
    current_state::{insert_entity, intern_attribute},
    database_catalog::{database_oid, first_migration_applied_at, last_migration_applied_at, stamp_instant},
    history::{InstanceIdentity, insert_history_row, insert_stream},
    ledgers::{last_tenant_migration, public_applied_through},
    registry::{PROVISIONAL, drop_registry_template, fresh_prefix, insert_v1_0_0_tenant, open_registry, open_tenant_pool},
    statements::count_rows,
    test_error::UpgradeTestError,
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_context_store::{
    tenant_database::{DatabaseCreation, DatabaseSource, create_database_if_missing},
    tenant_database_naming::derive_database_name,
    tenant_migration::{TenantSchemaState, tenant_schema_state},
    tenant_template::template_name::template_database_name,
};
use andromeda_tenancy::database::TenantDatabase;
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::{assert_eq, assert_ne};

#[tokio::test]
async fn tenants_copied_from_the_template_carry_the_whole_tenant_schema_and_stamps_of_their_own() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let registry = open_registry(&platform, &prefix).await?;

    let acme = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;
    let globex = registry.provision(&TenantName::try_from("globex".to_owned())?).await?;
    let acme_pool = open_tenant_pool(&server, &acme).await?;
    let globex_pool = open_tenant_pool(&server, &globex).await?;

    for pool in [&acme_pool, &globex_pool] {
        assert_eq!(public_applied_through(pool).await?, last_tenant_migration());
        assert_eq!(tenant_schema_state(pool).await?, TenantSchemaState::Current);
    }
    // Both copies keep the instants the template's ledger recorded, which two
    // databases migrated one after the other never share.
    assert_eq!(last_migration_applied_at(&acme_pool).await?, last_migration_applied_at(&globex_pool).await?);
    assert_ne!(stamp_instant(&acme_pool).await?, stamp_instant(&globex_pool).await?);

    acme_pool.close().await;
    globex_pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    server.drop_database(&globex).await?;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn tenants_provisioned_side_by_side_are_all_copied_from_the_one_template() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let registry = open_registry(&platform, &prefix).await?;
    let names = ["acme", "globex", "initech", "umbrella"]
        .into_iter()
        .map(|name| TenantName::try_from(name.to_owned()))
        .collect::<Result<Vec<TenantName>, _>>()?;

    let provisioned = tokio::try_join!(
        registry.provision(&names[0]),
        registry.provision(&names[1]),
        registry.provision(&names[2]),
        registry.provision(&names[3]),
    )?;

    let databases = [provisioned.0, provisioned.1, provisioned.2, provisioned.3];
    let mut applied_at = Vec::new();
    for database in &databases {
        let pool = open_tenant_pool(&server, database).await?;
        assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current, "{database}");
        applied_at.push(last_migration_applied_at(&pool).await?);
        pool.close().await;
    }
    applied_at.dedup();
    assert_eq!(applied_at.len(), 1, "{applied_at:?}");

    registry.main_pool().close().await;
    for database in &databases {
        server.drop_database(database).await?;
    }
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_tenant_database_an_earlier_provisioning_left_is_migrated_where_it_stands_and_never_copied() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    platform.migrate_platform_to_v1_0_0().await?;
    let prefix = fresh_prefix()?;
    let globex = TenantName::try_from("globex".to_owned())?;
    let left = UpgradeDatabase::create_named(&server, derive_database_name(&prefix, &globex)).await?;
    left.migrate_to_v1_0_0().await?;
    let left_first_migration = first_migration_applied_at(left.pool()).await?;
    let left_name = left.close().await;
    insert_v1_0_0_tenant(platform.pool(), "globex", left_name.as_str(), PROVISIONAL).await?;
    let oid_before = database_oid(platform.pool(), &left_name).await?;
    let registry = open_registry(&platform, &prefix).await?;

    assert_eq!(registry.provision(&globex).await?, left_name);

    let pool = open_tenant_pool(&server, &left_name).await?;
    assert_eq!(database_oid(platform.pool(), &left_name).await?, oid_before);
    assert_eq!(first_migration_applied_at(&pool).await?, left_first_migration);
    assert_eq!(public_applied_through(&pool).await?, last_tenant_migration());
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);

    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&left_name).await?;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_copy_whose_provisioning_stopped_before_its_stamp_was_renewed_is_stamped_anew() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let registry = open_registry(&platform, &prefix).await?;
    let globex = TenantName::try_from("globex".to_owned())?;
    let left = derive_database_name(&prefix, &globex);
    let copied = create_database_if_missing(server.url(), &left, DatabaseSource::Template(&template_database_name(&prefix))).await?;
    assert_eq!(copied, DatabaseCreation::Copied);
    let left_pool = open_tenant_pool(&server, &left).await?;
    let template_instant = stamp_instant(&left_pool).await?;
    left_pool.close().await;
    insert_v1_0_0_tenant(platform.pool(), "globex", left.as_str(), PROVISIONAL).await?;

    assert_eq!(registry.provision(&globex).await?, left);

    let pool = open_tenant_pool(&server, &left).await?;
    assert_ne!(stamp_instant(&pool).await?, template_instant);
    assert_eq!(tenant_schema_state(&pool).await?, TenantSchemaState::Current);

    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&left).await?;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_copy_records_temporal_history_in_chunks_of_its_own() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let platform = UpgradeDatabase::create(&server).await?;
    let prefix = fresh_prefix()?;
    let registry = open_registry(&platform, &prefix).await?;
    let acme: TenantDatabaseName = registry.provision(&TenantName::try_from("acme".to_owned())?).await?;
    let pool = open_tenant_pool(&server, &acme).await?;

    let attribute_key = intern_attribute(&pool, "https://example.org/temperature").await?;
    let entity_key = insert_entity(&pool, "urn:ngsi-ld:Sensor:1").await?;
    let stream_key = insert_stream(&pool, entity_key, attribute_key).await?;
    insert_history_row(&pool, stream_key, &InstanceIdentity::Generated("00000000-0000-4000-8000-0000000000b1"), &[]).await?;

    assert_eq!(count_rows(&pool, "attribute_history").await?, 1);

    pool.close().await;
    registry.main_pool().close().await;
    server.drop_database(&acme).await?;
    drop_registry_template(&server, &prefix).await?;
    platform.drop_database().await?;
    Ok(())
}
