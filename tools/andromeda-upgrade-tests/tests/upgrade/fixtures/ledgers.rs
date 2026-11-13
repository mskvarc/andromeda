//! What the migration ledgers of a database record, and the versions the
//! embedded migration sets end at.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_context_store::migration_set::{
    branch::{branch_ledger, branch_migrations},
    migration_version::MigrationVersion,
    platform::{PLATFORM_LEDGER, platform_migrations},
    tenant::{TENANT_LEDGER, tenant_migrator},
};
use sqlx::{PgPool, migrate::Migrate};

/// The highest migration `ledger` records, `None` when it records none.
async fn applied_through(pool: &PgPool, ledger: &str) -> Result<Option<MigrationVersion>, UpgradeTestError> {
    let mut connection = pool.acquire().await?;
    let applied = connection.list_applied_migrations(ledger).await?;
    Ok(applied.iter().map(|migration| MigrationVersion::new(migration.version)).max())
}

/// The highest migration the tenant ledger of `public` records.
pub(crate) async fn public_applied_through(pool: &PgPool) -> Result<Option<MigrationVersion>, UpgradeTestError> {
    applied_through(pool, TENANT_LEDGER).await
}

/// The highest migration the platform ledger records.
pub(crate) async fn platform_applied_through(pool: &PgPool) -> Result<Option<MigrationVersion>, UpgradeTestError> {
    applied_through(pool, PLATFORM_LEDGER).await
}

/// The highest migration the ledger of the branch schema `branch` of the
/// Snapshot `key` records.
pub(crate) async fn branch_applied_through(pool: &PgPool, key: SnapshotKey, branch: SnapshotBranch) -> Result<Option<MigrationVersion>, UpgradeTestError> {
    applied_through(pool, &branch_ledger(key, branch)).await
}

/// The last migration of the tenant set this build embeds.
pub(crate) fn last_tenant_migration() -> Option<MigrationVersion> {
    tenant_migrator().iter().map(|migration| MigrationVersion::new(migration.version)).max()
}

/// The last platform migration this build embeds.
pub(crate) fn last_platform_migration() -> Option<MigrationVersion> {
    platform_migrations().iter().map(|migration| MigrationVersion::new(migration.version)).max()
}

/// The last branch migration this build embeds.
pub(crate) fn last_branch_migration() -> Option<MigrationVersion> {
    branch_migrations().iter().map(|migration| MigrationVersion::new(migration.version)).max()
}
