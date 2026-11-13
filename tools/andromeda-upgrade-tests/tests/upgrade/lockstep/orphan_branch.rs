//! A branch schema no control record names, which the expiry sweep drops, is
//! never migrated, so it keeps no tenant from opening, even one it could not
//! follow.

use crate::{
    fixtures::{
        ledgers::{branch_applied_through, last_tenant_migration, public_applied_through},
        rehearsal::{at_v1_0_0, intern_seal_attribute},
        test_error::UpgradeTestError,
    },
    lockstep::sealed_branch::{hold_sub_attributes_aside, seed_sealed_entity},
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_upgrade_tests::release_schema::{V1_0_0_LAST_BRANCH_MIGRATION, V1_0_0_LAST_MIGRATION};
use pretty_assertions::assert_eq;

/// The key of a branch schema no Snapshot's control record names.
const ORPHAN_KEY: i64 = 9_000_001;

#[tokio::test]
async fn a_branch_schema_no_control_record_names_never_keeps_the_tenant_from_opening() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let orphan = SnapshotKey::try_from(ORPHAN_KEY)?;
    database.build_branch_schema(orphan, SnapshotBranch::Published, V1_0_0_LAST_MIGRATION).await?;
    let pool = database.branch_pool(orphan, SnapshotBranch::Published).await?;
    seed_sealed_entity(&pool, keys, seal_key).await?;
    pool.close().await;
    hold_sub_attributes_aside(database.pool(), orphan, SnapshotBranch::Published).await?;

    database.migrate_to_current().await?;

    assert_eq!(public_applied_through(database.pool()).await?, last_tenant_migration());
    assert_eq!(
        branch_applied_through(database.pool(), orphan, SnapshotBranch::Published).await?,
        Some(V1_0_0_LAST_BRANCH_MIGRATION)
    );
    database.drop_database().await?;
    Ok(())
}
