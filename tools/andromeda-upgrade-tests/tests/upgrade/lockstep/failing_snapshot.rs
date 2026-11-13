//! A Snapshot branch that cannot follow the migrations stops the upgrade
//! there, before any later migration runs anywhere, names itself, keeps what
//! it held, and lets the upgrade resume once it is repaired.

use crate::{
    fixtures::{
        ledgers::{branch_applied_through, last_branch_migration, last_tenant_migration, public_applied_through},
        rehearsal::{Keys, at_v1_0_0, intern_seal_attribute},
        snapshot_records::insert_v1_0_0_snapshot,
        statements::name,
        test_error::UpgradeTestError,
    },
    lockstep::sealed_branch::{
        SEAL_CONVERSION,
        assert_converted,
        assert_held_seal_unconverted,
        hold_sub_attributes_aside,
        restore_sub_attributes,
        seed_sealed_entity,
    },
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::snapshot_schema;
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    lockstep_migration::snapshot_migration_error::SnapshotMigrationError,
    migration_set::branch::BRANCH_LEDGER,
};
use andromeda_postgresql_seaquery::command::Command;
use andromeda_upgrade_tests::{release_schema::V1_0_0_LAST_BRANCH_MIGRATION, upgrade_database::UpgradeDatabase, upgrade_database_error::UpgradeDatabaseError};
use pretty_assertions::assert_eq;
use sea_query::Table;

/// A v1.0.0 tenant whose two Snapshots hold seals: one that follows the
/// migrations, created first, and one whose sub-attributes are held aside,
/// so the seal conversion fails in it.
struct StoppingTenant {
    /// The database.
    database: UpgradeDatabase,
    /// The dictionary keys the seals name.
    keys: Keys,
    /// The key of the seal attribute.
    seal_key: i32,
    /// The published Snapshot that follows the migrations.
    following: SnapshotKey,
    /// The staging Snapshot the seal conversion fails in.
    failing: SnapshotKey,
}

/// Builds the [`StoppingTenant`].
async fn stopping_tenant() -> Result<StoppingTenant, UpgradeTestError> {
    let (database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let following = insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:following", SnapshotBranch::Published).await?;
    let failing = insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:failing", SnapshotBranch::Staging).await?;
    for (key, branch) in [(following, SnapshotBranch::Published), (failing, SnapshotBranch::Staging)] {
        let pool = database.branch_pool(key, branch).await?;
        seed_sealed_entity(&pool, keys, seal_key).await?;
        pool.close().await;
    }
    hold_sub_attributes_aside(database.pool(), failing, SnapshotBranch::Staging).await?;
    Ok(StoppingTenant {
        database,
        keys,
        seal_key,
        following,
        failing,
    })
}

/// Holds `refused` to the failure of the seal conversion in the staging
/// branch of the Snapshot `failing`.
fn assert_stopped_at_the_failing_branch(refused: Result<(), UpgradeDatabaseError>, failing: SnapshotKey) {
    match refused {
        Err(UpgradeDatabaseError::Store(ContextStoreError::SnapshotMigration(failure))) => match *failure {
            SnapshotMigrationError::MigrationFailed { branch, version, .. } => {
                assert_eq!((branch.key(), branch.branch(), version), (failing, SnapshotBranch::Staging, SEAL_CONVERSION));
                assert_eq!(branch.snapshot_id().as_str(), "urn:ngsi-ld:Snapshot:failing");
            }
            other @ (SnapshotMigrationError::LedgerUnreadable { .. }
            | SnapshotMigrationError::SearchPathNotSet { .. }
            | SnapshotMigrationError::SearchPathNotRestored { .. }) => {
                panic!("the migration must fail: {other}")
            }
        },
        other => panic!("the upgrade must stop at the failing Snapshot branch: {other:?}"),
    }
}

#[tokio::test]
async fn a_snapshot_branch_that_cannot_convert_its_seals_stops_the_upgrade_before_the_types_change() -> Result<(), UpgradeTestError> {
    let StoppingTenant {
        mut database,
        following,
        failing,
        ..
    } = stopping_tenant().await?;

    let refused = database.migrate_to_current().await;

    assert_stopped_at_the_failing_branch(refused, failing);
    assert_eq!(public_applied_through(database.pool()).await?, Some(SEAL_CONVERSION));
    assert_eq!(
        branch_applied_through(database.pool(), following, SnapshotBranch::Published).await?,
        Some(SEAL_CONVERSION)
    );
    assert_eq!(
        branch_applied_through(database.pool(), failing, SnapshotBranch::Staging).await?,
        Some(V1_0_0_LAST_BRANCH_MIGRATION)
    );
    let pool = database.branch_pool(failing, SnapshotBranch::Staging).await?;
    assert_held_seal_unconverted(&pool).await?;
    pool.close().await;
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_stopped_upgrade_resumes_once_the_snapshot_branch_can_follow() -> Result<(), UpgradeTestError> {
    let StoppingTenant {
        mut database,
        keys,
        seal_key,
        following,
        failing,
    } = stopping_tenant().await?;
    assert_stopped_at_the_failing_branch(database.migrate_to_current().await, failing);
    restore_sub_attributes(database.pool(), failing, SnapshotBranch::Staging).await?;

    database.migrate_to_current().await?;

    assert_eq!(public_applied_through(database.pool()).await?, last_tenant_migration());
    for (key, branch) in [(following, SnapshotBranch::Published), (failing, SnapshotBranch::Staging)] {
        let pool = database.branch_pool(key, branch).await?;
        assert_converted(&pool, keys, seal_key).await?;
        pool.close().await;
        assert_eq!(branch_applied_through(database.pool(), key, branch).await?, last_branch_migration());
    }
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_snapshot_branch_whose_ledger_cannot_be_read_stops_the_upgrade_before_any_migration() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let lost = insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:ledger-lost", SnapshotBranch::Published).await?;
    Table::drop()
        .table((snapshot_schema(lost, SnapshotBranch::Published), name(BRANCH_LEDGER)))
        .to_owned()
        .to_query()
        .execute(database.pool())
        .await?;
    let public_before = public_applied_through(database.pool()).await?;

    let refused = database.migrate_to_current().await;

    match refused {
        Err(UpgradeDatabaseError::Store(ContextStoreError::SnapshotMigration(failure))) => match *failure {
            SnapshotMigrationError::LedgerUnreadable { branch, .. } => {
                assert_eq!((branch.key(), branch.branch()), (lost, SnapshotBranch::Published));
            }
            other @ (SnapshotMigrationError::SearchPathNotSet { .. }
            | SnapshotMigrationError::MigrationFailed { .. }
            | SnapshotMigrationError::SearchPathNotRestored { .. }) => {
                panic!("the ledger read must fail: {other}")
            }
        },
        other => panic!("the upgrade must stop at the Snapshot branch without a ledger: {other:?}"),
    }
    assert_eq!(public_applied_through(database.pool()).await?, public_before);
    database.drop_database().await?;
    Ok(())
}
