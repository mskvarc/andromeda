//! A Snapshot created once its tenant is upgraded builds its branch through
//! every branch migration, against types the tenant migrations changed since
//! those migrations were written.

use crate::fixtures::{
    ledgers::{branch_applied_through, last_branch_migration},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    snapshot_records::snapshot_key_of,
    test_error::UpgradeTestError,
};
use andromeda_context_space::snapshot_branch::SnapshotBranch;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;
use andromeda_postgresql_context_store::snapshot::repo::SnapshotRepository;
use andromeda_snapshot::store::{InsertSnapshotParams, SnapshotStore};
use chrono::{TimeDelta, Utc};
use pretty_assertions::assert_eq;

/// The Snapshot created after the upgrade.
const CREATED: &str = "urn:ngsi-ld:Snapshot:created";

#[tokio::test]
async fn a_snapshot_created_after_the_upgrade_builds_its_branch_through_every_branch_migration() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    // With a seal attribute interned, the replayed seal conversion plans its
    // rewrites, so every function they call must still exist.
    intern_seal_attribute(database.pool()).await?;
    database.migrate_to_current().await?;

    let id: Identifier = CREATED.parse()?;
    let now = Utc::now();
    SnapshotRepository::new(database.pool())
        .create(InsertSnapshotParams {
            id: &id,
            snapshot_queries: None,
            snapshot_temporal_queries: None,
            query_context: JsonLiteral::null(),
            snapshot_lifetime: None,
            snapshot_priority: 5,
            endpoint: None,
            receiver_info: None,
            status: SnapshotStatus::Preparing,
            clone_source_id: None,
            queries_details: None,
            temporal_queries_details: None,
            created_at: now,
            modified_at: now,
            expires_at: now + TimeDelta::hours(1),
            last_used_at: None,
        })
        .await?;

    let key = snapshot_key_of(database.pool(), CREATED).await?;
    assert_eq!(
        branch_applied_through(database.pool(), key, SnapshotBranch::Staging).await?,
        last_branch_migration()
    );
    database.drop_database().await?;
    Ok(())
}
