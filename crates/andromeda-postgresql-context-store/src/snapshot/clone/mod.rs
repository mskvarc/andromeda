//! Copying one Snapshot branch schema into another (clone preparation).
//!
//! A clone freezes the source Snapshot's published branch into the target
//! Snapshot's staging branch. Because each branch is its own schema with its own
//! identity sequences, a row copies verbatim -- the same `entity_key` and
//! `stream_key` land in the target -- so there is no surrogate-key remap and no
//! join: each copied relation is one `INSERT ... SELECT` between the two
//! schemas, in foreign-key-dependency order. An identity column is copied
//! through `OVERRIDING SYSTEM VALUE` and its sequence is then continued past the
//! copied keys ([`identity_continuation`]); a stored generated column is left
//! out of the column list for the database to recompute.
//!
//! Clause 5.16.2.4 copies "All Entity and Temporal Evolution of Entity data" of
//! the source and nothing else, so the relations holding the source's
//! Snapshot-scoped subscriptions and their notification queue stay empty in the
//! clone: a copy would make the clone notify the source's subscribers, and send
//! the source's pending notifications a second time.
//!
//! A published Snapshot branch accepts writes, and each copy is its own
//! statement, so the copied relations of the source are locked `SHARE` before
//! the first copy: a writer cannot commit between two copies, and every relation
//! is copied as of the same moment. That is what keeps a copied discovery census
//! equal to the copied search rows it counts, and a copied membership row beside
//! the copied Entity it belongs to.

mod column_plan;
mod identity_continuation;

use crate::{
    error::ContextStoreError,
    snapshot::clone::{
        column_plan::{ColumnPlan, column_plan},
        identity_continuation::continue_identity,
    },
};
use sqlx::{Executor, PgConnection};

/// The branch relations a clone copies, in the order `BRANCH_TABLES` gives
/// them, which respects their foreign keys (parents before children).
const CLONED_TABLES: [&str; 13] = [
    "entities",
    "temporal_entities",
    "entity_documents",
    "entity_search",
    "entity_type_census",
    "entity_type_attribute_census",
    "attribute_census",
    "entity_type_members",
    "current_attribute_values",
    "temporal_attribute_streams",
    "attribute_history",
    "scope_history",
    "attribute_history_instance_claims",
];

/// Copy the Entity and temporal relations of `source_schema` into
/// `target_schema` and continue the target's identity sequences past them.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when locking the source, reading a relation's
/// columns, executing one of the copies or continuing a sequence fails.
pub(crate) async fn clone_branch(conn: &mut PgConnection, source_schema: &str, target_schema: &str) -> Result<(), ContextStoreError> {
    conn.execute(sqlx::query(sqlx::AssertSqlSafe(lock_sql(source_schema).as_str()))).await?;
    for table in CLONED_TABLES {
        let plan = column_plan(conn, target_schema, table).await?;
        if plan.is_empty() {
            continue;
        }
        let copy = copy_sql(source_schema, target_schema, table, &plan);
        conn.execute(sqlx::query(sqlx::AssertSqlSafe(copy.as_str()))).await?;
        for column in plan.identity_columns() {
            continue_identity(conn, source_schema, target_schema, table, column).await?;
        }
    }
    Ok(())
}

/// The statement locking every copied relation of the source against writers.
fn lock_sql(source_schema: &str) -> String {
    let tables: Vec<String> = CLONED_TABLES.iter().map(|table| format!("{source_schema}.{table}")).collect();
    format!("LOCK TABLE {} IN SHARE MODE", tables.join(", "))
}

/// The statement copying one relation verbatim from the source to the target.
fn copy_sql(source_schema: &str, target_schema: &str, table: &str, plan: &ColumnPlan) -> String {
    let columns = plan.quoted_columns();
    let overriding = if plan.overrides_system_value() { " OVERRIDING SYSTEM VALUE" } else { "" };
    format!("INSERT INTO {target_schema}.{table} ({columns}){overriding} SELECT {columns} FROM {source_schema}.{table}")
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{
        branch_schema::BRANCH_TABLES,
        clone::{
            CLONED_TABLES,
            column_plan::{IdentityGeneration, fixture::entity_key_plan},
            copy_sql,
            lock_sql,
        },
    };
    use pretty_assertions::assert_eq;

    /// The branch relations a clone leaves empty: the Snapshot-scoped
    /// subscriptions and their delivery state and queue.
    const UNCLONED_TABLES: [&str; 4] = ["subscriptions", "notification_events", "subscription_delivery_state", "notification_deliveries"];

    /// Every branch relation is either copied or deliberately left empty, and
    /// the copies keep the foreign-key order of the branch catalogue.
    #[test]
    fn the_cloned_relations_are_the_branch_relations_less_the_subscription_state_in_branch_order() {
        let expected: Vec<&str> = BRANCH_TABLES.iter().copied().filter(|table| !UNCLONED_TABLES.contains(table)).collect();
        assert_eq!(CLONED_TABLES.to_vec(), expected);
        for table in UNCLONED_TABLES {
            assert!(BRANCH_TABLES.contains(&table), "{table} is a branch relation");
        }
    }

    /// Clause 5.16.2.4 copies Entity and Temporal Evolution data only.
    #[test]
    fn a_clone_copies_no_subscription_or_notification_relation() {
        for table in UNCLONED_TABLES {
            assert!(!CLONED_TABLES.contains(&table), "{table} is not copied");
        }
        let lock = lock_sql("snap_2");
        assert!(!lock.contains("subscriptions"), "{lock}");
        assert!(!lock.contains("notification_"), "{lock}");
    }

    #[test]
    fn the_source_is_locked_share_across_every_copied_relation() {
        let lock = lock_sql("snap_2");
        assert!(lock.starts_with("LOCK TABLE snap_2.entities, snap_2.temporal_entities, "), "{lock}");
        assert!(lock.ends_with(", snap_2.attribute_history_instance_claims IN SHARE MODE"), "{lock}");
    }

    #[test]
    fn an_always_identity_is_copied_by_overriding_the_system_value() {
        assert_eq!(
            copy_sql("snap_2", "snap_3_stg", "entities", &entity_key_plan(Some(IdentityGeneration::Always))),
            "INSERT INTO snap_3_stg.entities (\"entity_key\", \"id\") OVERRIDING SYSTEM VALUE SELECT \"entity_key\", \"id\" FROM snap_2.entities"
        );
    }

    #[test]
    fn a_relation_without_an_always_identity_is_copied_as_is() {
        for identity in [None, Some(IdentityGeneration::ByDefault)] {
            assert_eq!(
                copy_sql("snap_2", "snap_3_stg", "entities", &entity_key_plan(identity)),
                "INSERT INTO snap_3_stg.entities (\"entity_key\", \"id\") SELECT \"entity_key\", \"id\" FROM snap_2.entities"
            );
        }
    }
}
