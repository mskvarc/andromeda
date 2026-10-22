//! Creating, upgrading, promoting and tearing down a Snapshot's branch schema.
//!
//! Each Snapshot owns its state in one dedicated PostgreSQL schema named after
//! its surrogate key, holding the branch relation set: `snap_<key>_stg`, the
//! private staging branch, while it prepares, renamed to `snap_<key>`, the
//! published branch, when its preparation ends. A Snapshot never has both, so
//! its creation runs the branch migrations once and its publication is a
//! rename that locks nothing beyond the schema itself.
//! A branch schema is created by the branch migration set (the same files
//! that create the live branch relations in `public`), run with the schema
//! first on the search path and recorded in the schema's own ledger, so every
//! column, default, check constraint, index, foreign key, storage parameter,
//! generated column and compression setting, and every name, is exactly the
//! live branch's. The same migrator later upgrades the schema
//! ([`crate::snapshot::branch_upgrade`]). History relations stay plain tables
//! in a branch schema; only the live branch converts them to hypertables.
//!
use crate::{error::ContextStoreError, migration_set::branch::BranchMigrationSet};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::{snapshot_schema, snapshot_schema_name};
use andromeda_postgresql_schema::{
    census::{AttributeCensus, EntityTypeAttributeCensus, EntityTypeCensus},
    current::{CurrentAttributes, Entities, EntitySearch, EntityTypeMembers},
    notification::{NotificationDeliveries, NotificationEvents},
    subscriptions::{SubscriptionDeliveryState, Subscriptions},
    temporal::{
        AttributeHistory,
        AttributeHistoryInstanceClaims,
        ScopeHistory,
        TemporalAttributeStreams,
        TemporalEntities,
        TemporalEntityTypeMembers,
        TemporalStreamShapes,
    },
};
use andromeda_postgresql_seaquery::{
    command::Command,
    introspection::current_setting,
    maintenance::MaintenanceCommand,
    relation_truncation::RelationTruncation,
    schema_definition::SchemaDefinition,
    setting::{Setting, SettingScope, set_config},
    statement::Statement,
};
use sea_query::{DynIden, IntoIden, Query, TableName};
use sqlx::PgConnection;

/// The branch relations every Snapshot schema holds, in an order that respects
/// intra-branch references, foreign keys and statement-level checks alike
/// (parents before children), which the relations a clone copies keep. The
/// branch migrations create them; this list only orders the truncation and
/// the copies.
pub(crate) fn branch_tables() -> [DynIden; 18] {
    [
        Entities::Table.into_iden(),
        TemporalEntities::Table.into_iden(),
        Subscriptions::Table.into_iden(),
        NotificationEvents::Table.into_iden(),
        EntitySearch::Table.into_iden(),
        EntityTypeCensus::Table.into_iden(),
        EntityTypeAttributeCensus::Table.into_iden(),
        AttributeCensus::Table.into_iden(),
        EntityTypeMembers::Table.into_iden(),
        CurrentAttributes::Table.into_iden(),
        TemporalEntityTypeMembers::Table.into_iden(),
        TemporalAttributeStreams::Table.into_iden(),
        TemporalStreamShapes::Table.into_iden(),
        AttributeHistory::Table.into_iden(),
        ScopeHistory::Table.into_iden(),
        AttributeHistoryInstanceClaims::Table.into_iden(),
        SubscriptionDeliveryState::Table.into_iden(),
        NotificationDeliveries::Table.into_iden(),
    ]
}

/// Create a branch schema by running the branch migration set inside it.
///
/// Runs on the caller's transaction: every branch migration becomes a
/// savepoint of it, so the schema, its relations and its ledger commit or
/// roll back together with the Snapshot's control record. The migrator takes
/// no advisory lock (the schema is new and invisible to every other session
/// until the transaction commits), and the search path is changed only for the
/// rest of the transaction, then put back.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the schema cannot be created, the search
/// path cannot be set or restored, or a branch migration fails.
pub(crate) async fn create_branch_schema(conn: &mut PgConnection, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), ContextStoreError> {
    SchemaDefinition::create(snapshot_schema(key, branch)).to_query().execute(&mut *conn).await?;
    let schema = snapshot_schema_name(key, branch);
    let previous = current_search_path(conn).await?;
    steer_search_path(conn, &branch_search_path(&schema), SettingScope::Transaction).await?;
    let mut migrator = BranchMigrationSet::embedded().migrator(&schema);
    migrator.set_locking(false);
    // `run_direct` rather than `run`: `run` goes through `Acquire`, whose
    // lifetime-generic bound does not hold for a borrowed connection inside the
    // `Send` futures the store's async-trait methods return.
    migrator.run_direct(None, conn, false).await?;
    steer_search_path(conn, &previous, SettingScope::Transaction).await
}

/// The search path a branch migration runs under: the branch schema first, so
/// its unqualified relations land there, then `public`, so the tenant-global
/// dictionaries, unit codes, functions and extension types resolve.
pub(crate) fn branch_search_path(schema: &str) -> String {
    format!("{schema}, public")
}

/// Read the connection's current search path.
async fn current_search_path(conn: &mut PgConnection) -> Result<String, ContextStoreError> {
    Ok(Query::select()
        .expr(current_setting(Setting::SearchPath))
        .to_owned()
        .to_query_scalar::<String>()
        .fetch_one(conn)
        .await?)
}

/// Set the connection's search path for `scope`.
pub(crate) async fn steer_search_path(conn: &mut PgConnection, search_path: &str, scope: SettingScope) -> Result<(), ContextStoreError> {
    Query::select()
        .expr(set_config(Setting::SearchPath, search_path, scope))
        .to_owned()
        .to_query()
        .execute(conn)
        .await?;
    Ok(())
}

/// Drop a branch schema and everything it holds.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the `DROP SCHEMA` fails.
pub(crate) async fn drop_branch_schema(conn: &mut PgConnection, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), ContextStoreError> {
    SchemaDefinition::drop_cascade(snapshot_schema(key, branch)).to_query().execute(conn).await?;
    Ok(())
}

/// Empty the staging branch in place, resetting its identity sequences.
///
/// Truncating every branch relation in one statement is foreign-key safe (all
/// of them are cleared together) and far cheaper than dropping and recreating
/// the schema, which every claim and restart would otherwise pay for.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the `TRUNCATE` fails.
pub(crate) async fn truncate_branch_schema(conn: &mut PgConnection, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), ContextStoreError> {
    RelationTruncation::new(branch_relations(&snapshot_schema(key, branch)))
        .to_query()
        .execute(conn)
        .await?;
    Ok(())
}

/// Gather the planner statistics of every relation in a branch.
///
/// A capture or a clone fills its staging branch in bulk, and autovacuum
/// analyses a relation only once its naptime has passed and its changes cross
/// the analyze threshold, so a branch read straight after its publication would
/// be planned on default selectivities. `ANALYZE` samples a bounded number of
/// rows per relation, so its cost does not grow with the branch.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the `ANALYZE` fails.
pub(crate) async fn analyze_branch_schema(conn: &mut PgConnection, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), ContextStoreError> {
    MaintenanceCommand::Analyze(branch_relations(&snapshot_schema(key, branch)))
        .to_query()
        .execute(conn)
        .await?;
    Ok(())
}

/// Every branch relation qualified by `schema`, in [`branch_tables`] order.
pub(crate) fn branch_relations(schema: &DynIden) -> Vec<TableName> {
    // clone: each qualified name owns its schema identifier.
    branch_tables().into_iter().map(|table| TableName::from((schema.clone(), table))).collect()
}

/// Publish a Snapshot's prepared branch by renaming its staging schema to the
/// published name, so the published branch keeps its derivable `snap_<key>`
/// name.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the rename fails.
pub(crate) async fn promote_staging_schema(conn: &mut PgConnection, key: SnapshotKey) -> Result<(), ContextStoreError> {
    SchemaDefinition::rename(snapshot_schema(key, SnapshotBranch::Staging), snapshot_schema(key, SnapshotBranch::Published))
        .to_query()
        .execute(conn)
        .await?;
    Ok(())
}

/// Publish an empty branch for a Snapshot whose preparation failed: whatever
/// its attempts staged is discarded, and the failed Snapshot stays addressable
/// over an empty dataset (clause 5.16.1.4 ends it in `failure`, it does not
/// remove it).
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the truncation or the rename fails.
pub(crate) async fn publish_empty_branch(conn: &mut PgConnection, key: SnapshotKey) -> Result<(), ContextStoreError> {
    truncate_branch_schema(conn, key, SnapshotBranch::Staging).await?;
    promote_staging_schema(conn, key).await
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::branch::branch_migrations,
        snapshot::branch_schema::{branch_relations, branch_search_path, branch_tables},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, TableName};

    #[test]
    fn every_branch_relation_is_qualified_by_its_schema_in_branch_order() {
        let schema = Alias::new("snap_7_stg").into_iden();
        let relations = branch_relations(&schema);
        assert_eq!(relations.len(), branch_tables().len());
        for (relation, table) in relations.into_iter().zip(branch_tables()) {
            assert_eq!(relation, TableName::from((Alias::new("snap_7_stg"), table)));
        }
    }

    #[test]
    fn a_branch_migration_resolves_its_own_schema_before_the_tenant_globals() {
        assert_eq!(branch_search_path("snap_7_stg"), "snap_7_stg, public");
    }

    /// The copy and truncation order names exactly the relations the branch
    /// migrations create, so neither leaves a branch relation behind.
    #[test]
    fn the_branch_table_order_covers_every_relation_the_branch_migrations_create() {
        let mut created: Vec<String> = branch_migrations()
            .iter()
            .flat_map(|migration| {
                migration
                    .sql
                    .as_str()
                    .lines()
                    .filter_map(|line| line.get(..13).filter(|head| head.eq_ignore_ascii_case("create table ")).map(|_| &line[13..]))
                    .filter_map(|rest| rest.split_whitespace().next())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        created.sort();
        let created = created;
        let mut ordered: Vec<String> = branch_tables().iter().map(ToString::to_string).collect();
        ordered.sort();
        let ordered = ordered;
        assert_eq!(ordered, created);
    }
}
