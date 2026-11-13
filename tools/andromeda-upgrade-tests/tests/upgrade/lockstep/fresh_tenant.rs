//! A tenant database no migration has reached holds no Snapshot, and takes
//! the whole tenant set at once.

use crate::fixtures::{
    ledgers::{last_tenant_migration, public_applied_through},
    test_error::UpgradeTestError,
};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::assert_eq;

#[tokio::test]
async fn a_database_no_migration_has_reached_takes_the_whole_tenant_set() -> Result<(), UpgradeTestError> {
    let mut database = UpgradeDatabase::create(&UpgradeServer::from_environment()?).await?;

    database.migrate_to_current().await?;

    assert_eq!(public_applied_through(database.pool()).await?, last_tenant_migration());
    database.drop_database().await?;
    Ok(())
}
