//! Copying one Snapshot branch schema into another (clone preparation).
//!
//! A clone freezes the source Snapshot's published branch into the target
//! Snapshot's staging branch. Because each branch is its own schema with its own
//! identity sequences, a row copies verbatim (the same `entity_key` and
//! `stream_key` land in the target), so there is no surrogate-key remap and no
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
//! statement, so the copy runs inside its caller's REPEATABLE READ transaction:
//! every statement reads the snapshot that transaction took, and every relation
//! is copied as of the same moment while writers on the source carry on. That
//! is what keeps a copied discovery census equal to the copied search rows it
//! counts, and a copied membership row beside the copied Entity it belongs to.

mod column_plan;
mod identity_continuation;

use crate::{
    error::ContextStoreError,
    snapshot::{
        branch_schema::branch_tables,
        clone::{column_plan::column_plan, identity_continuation::continue_identity},
    },
};
use andromeda_postgresql_schema::{
    notification::{NotificationDeliveries, NotificationDeliveryHolds, NotificationEvents},
    subscriptions::{SubscriptionDeliveryState, Subscriptions},
};
use andromeda_postgresql_seaquery::{command::Command, relation_copy::RelationCopy};
use sea_query::{DynIden, IntoIden, TableName};
use sqlx::PgConnection;

/// The branch relations a clone leaves empty: the Snapshot-scoped
/// subscriptions, their delivery state and their notification queue.
fn uncloned_tables() -> [DynIden; 5] {
    [
        Subscriptions::Table.into_iden(),
        NotificationEvents::Table.into_iden(),
        SubscriptionDeliveryState::Table.into_iden(),
        NotificationDeliveries::Table.into_iden(),
        NotificationDeliveryHolds::Table.into_iden(),
    ]
}

/// The branch relations a clone copies, in [`branch_tables`] order, which
/// respects their references (parents before children).
fn cloned_tables() -> Vec<DynIden> {
    let uncloned = uncloned_tables().map(|table| table.to_string());
    branch_tables().into_iter().filter(|table| !uncloned.contains(&table.to_string())).collect()
}

/// Copy the Entity and temporal relations of `source_schema` into
/// `target_schema` and continue the target's identity sequences past them.
///
/// `conn` must be inside a REPEATABLE READ transaction: under READ COMMITTED
/// each copy would read the source as of a different moment.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when reading a relation's columns, executing
/// one of the copies or continuing a sequence fails.
pub(crate) async fn clone_branch(conn: &mut PgConnection, source_schema: &DynIden, target_schema: &DynIden) -> Result<(), ContextStoreError> {
    for table in cloned_tables() {
        let plan = column_plan(conn, target_schema, &table).await?;
        if plan.is_empty() {
            continue;
        }
        // clone: each qualified name owns its schema and relation identifiers.
        let source = TableName::from((source_schema.clone(), table.clone()));
        let target = TableName::from((target_schema.clone(), table));
        // clone: the copy owns the names it renders; the continuations below
        // name them again.
        RelationCopy::new(source.clone(), target.clone(), plan.columns(), plan.copied_identity())
            .to_query()
            .execute(&mut *conn)
            .await?;
        for column in plan.identity_columns() {
            continue_identity(conn, &source, &target, &column).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{
        branch_schema::branch_tables,
        clone::{cloned_tables, uncloned_tables},
    };
    use pretty_assertions::assert_eq;

    /// The relation holding every current attribute instance.
    const CURRENT_ATTRIBUTES: &str = "current_attributes";

    fn names(tables: impl IntoIterator<Item = sea_query::DynIden>) -> Vec<String> {
        tables.into_iter().map(|table| table.to_string()).collect()
    }

    /// Every branch relation is either copied or deliberately left empty, and
    /// the copies keep the foreign-key order of the branch catalogue.
    #[test]
    fn the_cloned_relations_are_the_branch_relations_less_the_subscription_state_in_branch_order() {
        let uncloned = names(uncloned_tables());
        let expected: Vec<String> = names(branch_tables()).into_iter().filter(|table| !uncloned.contains(table)).collect();
        assert_eq!(names(cloned_tables()), expected);
        for table in &uncloned {
            assert!(names(branch_tables()).contains(table), "{table} is a branch relation");
        }
    }

    /// Clause 5.16.2.4 copies Entity and Temporal Evolution data only.
    #[test]
    fn a_clone_copies_no_subscription_or_notification_relation() {
        let cloned = names(cloned_tables());
        for table in names(uncloned_tables()) {
            assert!(!cloned.contains(&table), "{table} is not copied");
        }
    }

    /// The rows carry their own grain, so the copy needs no surrogate key to
    /// continue and no parent order: one statement copies every column, the
    /// sub-attribute tree included.
    #[test]
    fn the_clone_copies_the_instance_table() {
        assert!(names(cloned_tables()).contains(&CURRENT_ATTRIBUTES.to_owned()));
    }
}
