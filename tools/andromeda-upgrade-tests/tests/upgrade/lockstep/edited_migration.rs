//! A tenant database whose ledger records an applied branch migration other
//! than the one this build embeds is refused, even when nothing is left to
//! apply anywhere.

use crate::fixtures::{rehearsal::at_v1_0_0, snapshot_records::insert_v1_0_0_snapshot, statements::name, test_error::UpgradeTestError};
use andromeda_context_space::snapshot_branch::SnapshotBranch;
use andromeda_postgresql_context_store::{error::ContextStoreError, migration_set::tenant::TENANT_LEDGER};
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_upgrade_tests::{release_schema::V1_0_0_LAST_BRANCH_MIGRATION, upgrade_database_error::UpgradeDatabaseError};
use sea_query::{Expr, ExprTrait, Query};
use sqlx::migrate::MigrateError;

#[tokio::test]
async fn an_edited_applied_branch_migration_is_refused_with_nothing_left_to_apply() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    insert_v1_0_0_snapshot(&database, "urn:ngsi-ld:Snapshot:published", SnapshotBranch::Published).await?;
    database.migrate_to_current().await?;
    let edited = V1_0_0_LAST_BRANCH_MIGRATION;
    Query::update()
        .table(name(TENANT_LEDGER))
        .value(name("checksum"), Expr::val(vec![0_u8; 48]))
        .and_where(Expr::col(name("version")).eq(edited.get()))
        .to_owned()
        .to_query()
        .execute(database.pool())
        .await?;

    let refused = database.migrate_to_current().await;

    assert!(
        matches!(
            refused,
            Err(UpgradeDatabaseError::Store(ContextStoreError::Migration(MigrateError::VersionMismatch(version)))) if version == edited.get()
        ),
        "{refused:?}"
    );
    database.drop_database().await?;
    Ok(())
}
