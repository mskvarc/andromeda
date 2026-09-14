//! Creating, promoting and tearing down a Snapshot's branch schema.
//!
//! Each Snapshot owns its state in a dedicated PostgreSQL schema named after its
//! surrogate key (`snap_<key>` for the published branch, `snap_<key>_stg` for
//! the private preparation branch), holding the identical branch relation set.
//! A branch table is created with `LIKE public.<table> INCLUDING ALL`, so the
//! tenant migrations stay the single source of truth for every column, default,
//! check constraint, index, storage parameter, generated column and compression
//! setting; only the foreign keys, which `LIKE` does not copy, are restated here
//! -- the intra-branch ones pointing inside the schema and the dictionary ones
//! pointing at the tenant-global `public` tables. A hypertable source yields a
//! plain table, which is the intended shape for a bounded, read-mostly Snapshot
//! history.
//!
//! These are the only statements in the store that cannot be expressed through
//! sea-query: PostgreSQL exposes no builder for `CREATE SCHEMA`, `LIKE`,
//! `ALTER SCHEMA ... RENAME` or `DROP SCHEMA`. The identifiers they interpolate
//! are a fixed table catalogue and a schema name derived from a positive
//! integer key, never client input, so the rendered DDL carries nothing that
//! could inject.

use crate::error::ContextStoreError;
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::snapshot_schema_name;
use sqlx::{Executor, PgConnection};

/// The branch relations copied into every Snapshot schema, in an order that
/// lets a clone `INSERT ... SELECT` respect intra-branch foreign keys
/// (parents before children).
pub(crate) const BRANCH_TABLES: [&str; 15] = [
    "entities",
    "temporal_entities",
    "subscriptions",
    "notification_events",
    "entity_documents",
    "entity_search",
    "current_attribute_values",
    "temporal_attribute_streams",
    "attribute_history",
    "scope_history",
    "attribute_history_instance_claims",
    "subscription_delivery_state",
    "notification_deliveries",
    "pending_side_effects",
    "temporal_append_intents",
];

/// Where a branch foreign key's referenced table lives.
enum Referenced {
    /// The Snapshot's own branch schema (an intra-branch edge).
    Branch,
    /// The tenant-global `public` schema (a dictionary edge).
    Public,
}

/// One foreign key `LIKE` does not copy and the branch therefore restates.
struct BranchForeignKey {
    /// The referencing table within the branch schema.
    table: &'static str,
    /// The referencing columns.
    columns: &'static str,
    /// Which schema the referenced table lives in.
    referenced: Referenced,
    /// The referenced table.
    to_table: &'static str,
    /// The referenced columns.
    to_columns: &'static str,
    /// Whether the edge cascades on delete.
    cascade: bool,
}

/// The foreign keys every branch schema restates after `LIKE` creation.
const BRANCH_FOREIGN_KEYS: [BranchForeignKey; 12] = [
    BranchForeignKey {
        table: "entity_documents",
        columns: "entity_key",
        referenced: Referenced::Branch,
        to_table: "entities",
        to_columns: "entity_key",
        cascade: true,
    },
    BranchForeignKey {
        table: "entity_search",
        columns: "entity_key",
        referenced: Referenced::Branch,
        to_table: "entities",
        to_columns: "entity_key",
        cascade: true,
    },
    BranchForeignKey {
        table: "current_attribute_values",
        columns: "entity_key",
        referenced: Referenced::Branch,
        to_table: "entities",
        to_columns: "entity_key",
        cascade: true,
    },
    BranchForeignKey {
        table: "current_attribute_values",
        columns: "attribute_name_id",
        referenced: Referenced::Public,
        to_table: "attribute_names",
        to_columns: "id",
        cascade: false,
    },
    BranchForeignKey {
        table: "current_attribute_values",
        columns: "unit_code_id",
        referenced: Referenced::Public,
        to_table: "unit_codes",
        to_columns: "id",
        cascade: false,
    },
    BranchForeignKey {
        table: "temporal_attribute_streams",
        columns: "temporal_entity_key",
        referenced: Referenced::Branch,
        to_table: "temporal_entities",
        to_columns: "entity_key",
        cascade: true,
    },
    BranchForeignKey {
        table: "temporal_attribute_streams",
        columns: "attribute_name_id",
        referenced: Referenced::Public,
        to_table: "attribute_names",
        to_columns: "id",
        cascade: false,
    },
    BranchForeignKey {
        table: "attribute_history_instance_claims",
        columns: "temporal_entity_key",
        referenced: Referenced::Branch,
        to_table: "temporal_entities",
        to_columns: "entity_key",
        cascade: true,
    },
    BranchForeignKey {
        table: "attribute_history_instance_claims",
        columns: "attribute_name_id",
        referenced: Referenced::Public,
        to_table: "attribute_names",
        to_columns: "id",
        cascade: false,
    },
    BranchForeignKey {
        table: "subscription_delivery_state",
        columns: "subscription_key",
        referenced: Referenced::Branch,
        to_table: "subscriptions",
        to_columns: "subscription_key",
        cascade: true,
    },
    BranchForeignKey {
        table: "notification_deliveries",
        columns: "event_id",
        referenced: Referenced::Branch,
        to_table: "notification_events",
        to_columns: "id",
        cascade: true,
    },
    BranchForeignKey {
        table: "notification_deliveries",
        columns: "subscription_key",
        referenced: Referenced::Branch,
        to_table: "subscriptions",
        to_columns: "subscription_key",
        cascade: true,
    },
];

/// Create an empty branch schema holding the identical branch relation set.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when any of the schema, table or foreign-key
/// statements fails.
pub(crate) async fn create_branch_schema(conn: &mut PgConnection, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), ContextStoreError> {
    let schema = snapshot_schema_name(key, branch);
    exec_ddl(conn, format!("CREATE SCHEMA {schema}")).await?;
    for table in BRANCH_TABLES {
        exec_ddl(conn, format!("CREATE TABLE {schema}.{table} (LIKE public.{table} INCLUDING ALL)")).await?;
    }
    for foreign_key in &BRANCH_FOREIGN_KEYS {
        exec_ddl(conn, foreign_key.render(&schema)).await?;
    }
    Ok(())
}

/// Execute one branch-DDL statement built from the trusted table catalogue and
/// a schema name derived from a positive key -- never client input, so the
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
/// the schema, which a fresh first claim would otherwise pay for.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the `TRUNCATE` fails.
pub(crate) async fn truncate_branch_schema(conn: &mut PgConnection, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), ContextStoreError> {
    let schema = snapshot_schema_name(key, branch);
    let qualified: Vec<String> = BRANCH_TABLES.iter().map(|table| format!("{schema}.{table}")).collect();
    exec_ddl(conn, format!("TRUNCATE {} RESTART IDENTITY CASCADE", qualified.join(", "))).await
}

/// Publish a Snapshot's captured branch: drop the empty published schema and
/// rename the staging schema into its place, so the published branch keeps its
/// derivable `snap_<key>` name.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when either schema statement fails.
pub(crate) async fn promote_staging_schema(conn: &mut PgConnection, key: SnapshotKey) -> Result<(), ContextStoreError> {
    let published = snapshot_schema_name(key, SnapshotBranch::Published);
    let staging = snapshot_schema_name(key, SnapshotBranch::Staging);
    exec_ddl(conn, format!("DROP SCHEMA IF EXISTS {published} CASCADE")).await?;
    exec_ddl(conn, format!("ALTER SCHEMA {staging} RENAME TO {published}")).await
}

impl BranchForeignKey {
    /// Render the `ALTER TABLE ... ADD FOREIGN KEY` that restates this edge in `schema`.
    fn render(&self, schema: &str) -> String {
        let referenced_schema = match self.referenced {
            Referenced::Branch => schema,
            Referenced::Public => "public",
        };
        let on_delete = if self.cascade { " ON DELETE CASCADE" } else { "" };
        format!(
            "ALTER TABLE {schema}.{table} ADD FOREIGN KEY ({columns}) REFERENCES {referenced_schema}.{to_table} ({to_columns}){on_delete}",
            table = self.table,
            columns = self.columns,
            to_table = self.to_table,
            to_columns = self.to_columns,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot::branch_schema::{BRANCH_FOREIGN_KEYS, BRANCH_TABLES, BranchForeignKey, Referenced};
    use pretty_assertions::assert_eq;

    #[test]
    fn every_foreign_key_names_a_branch_table_on_both_ends_it_should() {
        for foreign_key in &BRANCH_FOREIGN_KEYS {
            assert!(
                BRANCH_TABLES.contains(&foreign_key.table),
                "referencing table {} is a branch table",
                foreign_key.table
            );
            if matches!(foreign_key.referenced, Referenced::Branch) {
                assert!(
                    BRANCH_TABLES.contains(&foreign_key.to_table),
                    "intra-branch target {} is a branch table",
                    foreign_key.to_table
                );
            }
        }
    }

    #[test]
    fn an_intra_branch_key_qualifies_both_tables_with_the_branch_schema() {
        let foreign_key = BranchForeignKey {
            table: "entity_documents",
            columns: "entity_key",
            referenced: Referenced::Branch,
            to_table: "entities",
            to_columns: "entity_key",
            cascade: true,
        };
        assert_eq!(
            foreign_key.render("snap_7"),
            "ALTER TABLE snap_7.entity_documents ADD FOREIGN KEY (entity_key) REFERENCES snap_7.entities (entity_key) ON DELETE CASCADE"
        );
    }

    #[test]
    fn a_dictionary_key_points_at_the_public_schema() {
        let foreign_key = BranchForeignKey {
            table: "current_attribute_values",
            columns: "attribute_name_id",
            referenced: Referenced::Public,
            to_table: "attribute_names",
            to_columns: "id",
            cascade: false,
        };
        assert_eq!(
            foreign_key.render("snap_7"),
            "ALTER TABLE snap_7.current_attribute_values ADD FOREIGN KEY (attribute_name_id) REFERENCES public.attribute_names (id)"
        );
    }
}
