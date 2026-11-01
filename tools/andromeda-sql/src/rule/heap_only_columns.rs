//! `heap-only-columns`: the columns rewritten on almost every write of their
//! row stay out of every index, so the update stays heap-only.
//!
//! PostgreSQL updates a row in place on its page, touching no index, only
//! while no indexed column changes. The listed columns change on nearly every
//! write of their row: an Entity's two modification instants, the delivery
//! counters of the subscription and registration delivery state, and who
//! claimed a queue entry (a queue's lease end is its claim order and is
//! indexed on purpose). An index naming one of them in a key, an
//! expression, a predicate or an `include` list turns each of those writes
//! into a write of every index of the table.

use crate::rule::{Rule, RuleContext, RuleFinding};

/// The columns whose updates stay heap-only, as `(table, column)`.
const HEAP_ONLY_COLUMNS: &[(&str, &str)] = &[
    ("entities", "modified_at"),
    ("entities", "evolution_modified_at"),
    ("subscription_delivery_state", "times_sent"),
    ("subscription_delivery_state", "times_failed"),
    ("registration_delivery_state", "times_sent"),
    ("registration_delivery_state", "times_failed"),
    ("notification_deliveries", "claimed_by"),
    ("snapshot_preparation", "claimed_by"),
    ("snapshot_notification_outbox", "claimed_by"),
    ("pending_side_effects", "claimed_by"),
];

/// An index naming a heap-only column.
#[derive(Debug, derive_more::Display)]
#[display("{index} names {table}.{column}, whose every update would then write the index")]
struct IndexedHeapOnlyColumn<'schema> {
    /// The index.
    index: &'schema str,
    /// The table.
    table: &'schema str,
    /// The column.
    column: &'schema str,
}

/// Every index naming a heap-only column of its table.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .indexes()
        .iter()
        .flat_map(|index| {
            let mut named = index
                .referenced_columns()
                .iter()
                .chain(index.included())
                .map(String::as_str)
                .collect::<Vec<_>>();
            named.sort_unstable();
            named.dedup();
            named
                .into_iter()
                .filter(|column| HEAP_ONLY_COLUMNS.contains(&(index.table(), column)))
                .map(|column| {
                    context.finding(
                        Rule::HeapOnlyColumns,
                        index.origin(),
                        index.name().len(),
                        &IndexedHeapOnlyColumn {
                            index: index.name(),
                            table: index.table(),
                            column,
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_entities.sql";

    const TABLE: &str =
        "create table entities (\n    entity_key bigint not null,\n    created_at timestamptz not null,\n    modified_at timestamptz not null\n);\n\n";

    #[test]
    fn an_index_of_other_columns_is_accepted() -> Result<(), FixtureError> {
        let sql = format!("{TABLE}create index idx_entities_created_at on entities (created_at);\n");
        assert_eq!(findings(Rule::HeapOnlyColumns, &[(FILE, &sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_heap_only_column_in_a_predicate_is_refused_at_the_index() -> Result<(), FixtureError> {
        let sql = format!("{TABLE}create index idx_entities_created_at on entities (created_at)\nwhere\n    modified_at > created_at;\n");
        assert_eq!(findings(Rule::HeapOnlyColumns, &[(FILE, &sql)])?, ["migrations/branch/0001_entities.sql:7:14"]);
        Ok(())
    }
}
