//! An upgrade cut short after `public` applied a branch migration resumes
//! with the Snapshot branches, before `public` moves past it.

use crate::{
    fixtures::{
        ledgers::{branch_applied_through, last_branch_migration},
        rehearsal::{at_v1_0_0, intern_seal_attribute},
        snapshot_records::insert_v1_0_0_snapshot,
        test_error::UpgradeTestError,
    },
    lockstep::sealed_branch::{SEAL_CONVERSION, assert_converted, seed_sealed_entity},
};
use andromeda_context_space::snapshot_branch::SnapshotBranch;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn a_snapshot_branch_behind_public_converts_its_seals_before_the_types_change() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let published = insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:published", SnapshotBranch::Published).await?;
    let pool = database.branch_pool(published, SnapshotBranch::Published).await?;
    seed_sealed_entity(&pool, keys, seal_key).await?;
    pool.close().await;
    database.migrate_public_through(SEAL_CONVERSION).await?;

    database.migrate_to_current().await?;

    let pool = database.branch_pool(published, SnapshotBranch::Published).await?;
    assert_converted(&pool, keys, seal_key).await?;
    pool.close().await;
    assert_eq!(
        branch_applied_through(database.pool(), published, SnapshotBranch::Published).await?,
        last_branch_migration()
    );
    database.drop_database().await?;
    Ok(())
}
