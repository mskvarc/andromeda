//! The upgrade leaves no `query_name_rewrite` schema, a Snapshot created
//! after it builds its branch through the branch rewrite over an empty
//! `subscriptions`, and a tenant that stored no Subscription and no Snapshot
//! upgrades with nothing changed.

use crate::fixtures::{
    ledgers::{branch_applied_through, last_branch_migration},
    rehearsal::at_v1_0_0,
    snapshot_records::snapshot_key_of,
    statements::{count_rows, name},
    test_error::UpgradeTestError,
};
use andromeda_context_space::snapshot_branch::SnapshotBranch;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;
use andromeda_postgresql_context_store::snapshot::repo::SnapshotRepository;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_snapshot::store::{InsertSnapshotParams, SnapshotStore};
use chrono::{TimeDelta, Utc};
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query};
use sqlx::PgPool;

/// The schema the rewrite function lives in.
const REWRITE_SCHEMA: &str = "query_name_rewrite";

/// The Snapshot created after the upgrade.
const CREATED: &str = "urn:ngsi-ld:Snapshot:created-after-the-name-rewrite";

/// The schemas of the database named like the rewrite schema.
async fn rewrite_schemas(pool: &PgPool) -> Result<Vec<String>, UpgradeTestError> {
    let select = Query::select()
        .column(name("nspname"))
        .from((name("pg_catalog"), name("pg_namespace")))
        .and_where(Expr::col(name("nspname")).eq(REWRITE_SCHEMA))
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_all(pool).await?)
}

#[tokio::test]
async fn the_upgrade_leaves_no_rewrite_schema_and_a_snapshot_created_after_it_builds_its_branch() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    database.migrate_to_current().await?;
    assert_eq!(rewrite_schemas(database.pool()).await?, Vec::<String>::new());

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

#[tokio::test]
async fn a_tenant_holding_no_subscription_and_no_snapshot_upgrades_with_nothing_changed() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;

    database.migrate_to_current().await?;

    assert_eq!(count_rows(database.pool(), "subscriptions").await?, 0);
    assert_eq!(count_rows(database.pool(), "snapshots").await?, 0);
    assert_eq!(rewrite_schemas(database.pool()).await?, Vec::<String>::new());
    database.drop_database().await?;
    Ok(())
}
