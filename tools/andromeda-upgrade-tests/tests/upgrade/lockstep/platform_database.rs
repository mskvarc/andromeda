//! The platform database, which is also the default tenant's, runs the
//! platform set and then brings its Snapshot branches through the migrations
//! with `public`, as every other tenant database does.

use crate::{
    fixtures::{
        ledgers::{
            branch_applied_through,
            last_branch_migration,
            last_platform_migration,
            last_tenant_migration,
            platform_applied_through,
            public_applied_through,
        },
        rehearsal::{intern_seal_attribute, platform_at_v1_0_0},
        snapshot_records::insert_v1_0_0_snapshot,
        test_error::UpgradeTestError,
    },
    lockstep::sealed_branch::{assert_converted, seed_sealed_entity},
};
use andromeda_context_space::snapshot_branch::SnapshotBranch;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn the_platform_database_converts_the_seals_of_its_snapshot_branches_as_its_live_branch_does() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = platform_at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    seed_sealed_entity(database.pool(), keys, seal_key).await?;
    let snapshot = insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:default-tenant", SnapshotBranch::Published).await?;
    let pool = database.branch_pool(snapshot, SnapshotBranch::Published).await?;
    seed_sealed_entity(&pool, keys, seal_key).await?;
    pool.close().await;

    database.migrate_platform_to_current().await?;

    assert_eq!(platform_applied_through(database.pool()).await?, last_platform_migration());
    assert_eq!(public_applied_through(database.pool()).await?, last_tenant_migration());
    assert_converted(database.pool(), keys, seal_key).await?;
    let pool = database.branch_pool(snapshot, SnapshotBranch::Published).await?;
    assert_converted(&pool, keys, seal_key).await?;
    pool.close().await;
    assert_eq!(
        branch_applied_through(database.pool(), snapshot, SnapshotBranch::Published).await?,
        last_branch_migration()
    );
    database.drop_database().await?;
    Ok(())
}
