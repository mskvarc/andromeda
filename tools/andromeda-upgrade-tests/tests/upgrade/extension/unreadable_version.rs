//! A tenant database whose catalogue records a `PostGIS` version no version
//! reads, as `postgis_extensions_upgrade` leaves it while it runs. The
//! broker's probe and its extension reconciliation refuse the database naming
//! the extension, rather than read it as some version.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    extension_update::update_extensions,
    schema_contract::{error::SchemaContractError, versioned_extension::VersionedExtension},
    tenant_migration::tenant_schema_state,
    tracked_extension::TrackedExtension,
};
use andromeda_postgresql_seaquery::{command::Command, test_extension::InstalledVersionRecord};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use sea_query::Alias;

/// Whether `outcome` is the refusal naming `PostGIS`.
fn names_postgis<T>(outcome: &Result<T, ContextStoreError>) -> bool {
    matches!(
        outcome,
        Err(ContextStoreError::SchemaContract(SchemaContractError::ExtensionVersionValue {
            extension: TrackedExtension::Updated(VersionedExtension::PostGis),
            ..
        }))
    )
}

#[tokio::test]
async fn a_postgis_version_recorded_as_any_is_refused_naming_postgis() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let mut database = UpgradeDatabase::create(&server).await?;
    database.migrate_to_current().await?;
    let url = server.url().with_database_name(database.database_name());
    InstalledVersionRecord::new(Alias::new("postgis"), "ANY")
        .to_query()
        .execute(database.pool())
        .await?;

    let probed = tenant_schema_state(database.pool()).await;
    let reconciled = update_extensions(&url).await;

    assert!(names_postgis(&probed), "the probe refuses naming postgis: {probed:?}");
    assert!(names_postgis(&reconciled), "the reconciliation refuses naming postgis: {reconciled:?}");
    database.drop_database().await?;
    Ok(())
}
