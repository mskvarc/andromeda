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
//! The statements here that sea-query cannot express (`CREATE SCHEMA`,
//! `ALTER SCHEMA ... RENAME`, `DROP SCHEMA`, `TRUNCATE`) interpolate a fixed
//! table catalogue and a schema name derived from a positive integer key, never
//! client input, so the rendered DDL carries nothing that could inject.

use crate::{error::ContextStoreError, migration_set::branch::BranchMigrationSet};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::snapshot_schema_name;
use andromeda_postgresql_seaquery::{
    introspection::current_setting,
    setting::{SettingScope, set_config},
};
use sea_query::{PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Executor, PgConnection};

/// The runtime setting a branch migration is steered into its schema by.
const SEARCH_PATH: &str = "search_path";

/// The branch relations every Snapshot schema holds, in an order that respects
/// intra-branch references, foreign keys and statement-level checks alike
/// (parents before children), which the relations a clone copies keep. The branch migrations create them; this list only orders
/// the truncation and the copies.
pub(crate) const BRANCH_TABLES: [&str; 17] = [
    "entities",
    "temporal_entities",
    "subscriptions",
    "notification_events",
    "entity_search",
    "entity_type_census",
    "entity_type_attribute_census",
    "attribute_census",
    "entity_type_members",
    "current_attributes",
    "temporal_entity_type_members",
    "temporal_attribute_streams",
    "attribute_history",
    "scope_history",
    "attribute_history_instance_claims",
    "subscription_delivery_state",
    "notification_deliveries",
];

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
    let schema = snapshot_schema_name(key, branch);
    exec_ddl(conn, format!("CREATE SCHEMA {schema}")).await?;
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
    let (sql, values) = Query::select().expr(current_setting(SEARCH_PATH)).build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with::<_, String, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(conn)
        .await?)
}

/// Set the connection's search path for `scope`.
pub(crate) async fn steer_search_path(conn: &mut PgConnection, search_path: &str, scope: SettingScope) -> Result<(), ContextStoreError> {
    let (sql, values) = Query::select()
        .expr(set_config(SEARCH_PATH, search_path, scope))
        .build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(conn).await?;
    Ok(())
}

/// Execute one branch-DDL statement built from the trusted table catalogue and
/// a schema name derived from a positive key, never client input, so the
/// dynamic text is asserted safe.
async fn exec_ddl(conn: &mut PgConnection, sql: String) -> Result<(), ContextStoreError> {
    conn.execute(sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))).await?;
    Ok(())
}

/// Drop a branch schema and everything it holds.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the `DROP SCHEMA` fails.
pub(crate) async fn drop_branch_schema(conn: &mut PgConnection, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), ContextStoreError> {
    let schema = snapshot_schema_name(key, branch);
    exec_ddl(conn, format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).await
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
    let schema = snapshot_schema_name(key, branch);
    let qualified: Vec<String> = BRANCH_TABLES.iter().map(|table| format!("{schema}.{table}")).collect();
    exec_ddl(conn, format!("TRUNCATE {} RESTART IDENTITY CASCADE", qualified.join(", "))).await
}

/// Publish a Snapshot's prepared branch by renaming its staging schema to the
/// published name, so the published branch keeps its derivable `snap_<key>`
/// name.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the rename fails.
pub(crate) async fn promote_staging_schema(conn: &mut PgConnection, key: SnapshotKey) -> Result<(), ContextStoreError> {
    let published = snapshot_schema_name(key, SnapshotBranch::Published);
    let staging = snapshot_schema_name(key, SnapshotBranch::Staging);
    exec_ddl(conn, format!("ALTER SCHEMA {staging} RENAME TO {published}")).await
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
        snapshot::branch_schema::{BRANCH_TABLES, branch_search_path},
    };
    use pretty_assertions::assert_eq;

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
        let mut ordered: Vec<String> = BRANCH_TABLES.iter().map(|table| (*table).to_owned()).collect();
        ordered.sort();
        let ordered = ordered;
        assert_eq!(ordered, created);
    }
}
