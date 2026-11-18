//! A tenant v1.0.0 registered is served from the database the registry stores
//! for it, whether its provisioning finished or not, and a tenant registered
//! after the upgrade whose label it shares gets a database of its own.

use crate::fixtures::{
    registry::{ACTIVE, PROVISIONAL, drop_registry_template, fresh_prefix, insert_v1_0_0_tenant, open_registry},
    test_error::UpgradeTestError,
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_context_store::tenant_database_naming::derive_database_name;
use andromeda_tenancy::{database::TenantDatabase, registered_tenant::RegisteredTenant};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::assert_eq;

/// The suffix v1.0.0 put after the label of the tenant `acme`: the low 32 bits
/// of the standard library's default hash of the name, in hexadecimal.
const ACME_V1_0_0_SUFFIX: &str = "8e086db9";

/// The suffix v1.0.0 put after the label of the tenant `globex`, read as
/// [`ACME_V1_0_0_SUFFIX`] is.
const GLOBEX_V1_0_0_SUFFIX: &str = "cd13640c";

#[tokio::test]
async fn tenants_v1_0_0_registered_keep_their_database_names_and_a_new_tenant_sharing_a_label_gets_its_own() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let database = UpgradeDatabase::create(&server).await?;
    database.migrate_platform_to_v1_0_0().await?;
    // The prefix keeps the databases the registry creates apart from every
    // other rehearsal's, and v1.0.0 put the same suffix after any prefix.
    let prefix = fresh_prefix()?;
    let acme_database = format!("{prefix}_acme_{ACME_V1_0_0_SUFFIX}");
    let globex_database = format!("{prefix}_globex_{GLOBEX_V1_0_0_SUFFIX}");
    insert_v1_0_0_tenant(database.pool(), "acme", &acme_database, ACTIVE).await?;
    insert_v1_0_0_tenant(database.pool(), "globex", &globex_database, PROVISIONAL).await?;

    let registry = open_registry(&database, &prefix).await?;

    let acme = TenantName::try_from("acme".to_owned())?;
    let registered = registry.registered_tenant(&acme).await?;
    assert!(
        matches!(&registered, Some(RegisteredTenant::Established(record)) if record.database.as_str() == acme_database),
        "{registered:?}"
    );
    assert_eq!(registry.provision(&acme).await?.as_str(), acme_database);
    let globex = TenantName::try_from("globex".to_owned())?;
    assert_eq!(registry.provision(&globex).await?.as_str(), globex_database);
    let new_acme = TenantName::try_from("Acme".to_owned())?;
    let new_acme_database = registry.provision(&new_acme).await?;
    assert_eq!(new_acme_database, derive_database_name(&prefix, &new_acme));
    assert_eq!(new_acme_database.as_str(), format!("{prefix}_acme_37036cd8f9746d33"));

    registry.main_pool().close().await;
    server.drop_database(&TenantDatabaseName::from(globex_database)).await?;
    server.drop_database(&new_acme_database).await?;
    drop_registry_template(&server, &prefix).await?;
    database.drop_database().await?;
    Ok(())
}
