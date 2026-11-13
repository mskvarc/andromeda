//! A v1.0.0 tenant's Snapshot branches, published and staging alike, convert
//! their seals as the live branch does, and every schema of the database ends
//! at one version.

use crate::{
    fixtures::{
        ledgers::{branch_applied_through, last_branch_migration, last_tenant_migration, public_applied_through},
        rehearsal::{at_v1_0_0, intern_seal_attribute},
        snapshot_records::{insert_v1_0_0_snapshot, snapshot_preparation_columns},
        test_error::UpgradeTestError,
    },
    lockstep::sealed_branch::{assert_converted, seed_sealed_entity},
};
use andromeda_context_space::snapshot_branch::SnapshotBranch;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn every_snapshot_branch_converts_its_seals_as_the_live_branch_does() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    seed_sealed_entity(database.pool(), keys, seal_key).await?;
    let published = insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:published", SnapshotBranch::Published).await?;
    let staging = insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:staging", SnapshotBranch::Staging).await?;
    let branches = [(published, SnapshotBranch::Published), (staging, SnapshotBranch::Staging)];
    for (key, branch) in branches {
        let pool = database.branch_pool(key, branch).await?;
        seed_sealed_entity(&pool, keys, seal_key).await?;
        pool.close().await;
    }

    database.migrate_to_current().await?;

    assert_converted(database.pool(), keys, seal_key).await?;
    assert_eq!(public_applied_through(database.pool()).await?, last_tenant_migration());
    for (key, branch) in branches {
        let pool = database.branch_pool(key, branch).await?;
        assert_converted(&pool, keys, seal_key).await?;
        pool.close().await;
        assert_eq!(branch_applied_through(database.pool(), key, branch).await?, last_branch_migration());
    }
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn no_snapshot_records_a_branch_version_or_an_upgrade_failure_of_its_own() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:published", SnapshotBranch::Published).await?;

    database.migrate_to_current().await?;

    let columns = snapshot_preparation_columns(database.pool()).await?;
    // The key is always there, so a read that found no column cannot pass.
    assert!(columns.iter().any(|column| column == "snapshot_key"), "{columns:?}");
    for dropped in ["branch_schema_version", "branch_upgrade_error"] {
        assert!(!columns.iter().any(|column| column == dropped), "{dropped} is still a column: {columns:?}");
    }
    database.drop_database().await?;
    Ok(())
}
