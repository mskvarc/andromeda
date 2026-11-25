//! `fixed-width-before-variable`: in a table, no fixed-width column follows a
//! variable-length one.
//!
//! PostgreSQL caches the offset of every column up to the first
//! variable-length one; a fixed-width column after it has to be found by
//! walking the row on every read. Columns are laid out in the order they are
//! declared, an added column last. Composite types are not held to it: their
//! field order is the order of the table columns they mirror. Neither is a
//! column on the list below, which every row reaches by walking anyway.

use crate::{
    declared_schema::{column::ColumnSource, declared_type::Storage, table::DeclaredTable},
    rule::{Rule, RuleContext, RuleFinding},
};
use std::collections::BTreeSet;

/// Fixed-width columns a migration adds after a variable-length one, which
/// every row reaches by walking whatever their place: PostgreSQL stops using
/// cached offsets at a row's first NULL, and a check of the table leaves every
/// row a NULL ahead of the variable-length column.
const WALKED_COLUMNS: &[(&str, &str)] = &[
    // `chk_notification_deliveries_exhausted_state` leaves every row NULL in
    // `delivered_at` or in `exhausted_at`, both ahead of `last_error`.
    ("notification_deliveries", "committed_at"),
    ("notification_deliveries", "event_ordinal"),
];

/// A fixed-width column placed after a variable-length one.
#[derive(Debug, derive_more::Display)]
#[display("{table}.{column} is fixed-width and follows the variable-length {after}")]
struct LateFixedColumn<'schema> {
    /// The table.
    table: &'schema str,
    /// The column.
    column: &'schema str,
    /// The first variable-length column before it.
    after: &'schema str,
}

/// Every fixed-width table column declared after a variable-length one.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let tables = schema.tables().iter().map(DeclaredTable::name).collect::<BTreeSet<_>>();
    tables
        .into_iter()
        .flat_map(|table| {
            let columns = schema
                .columns()
                .iter()
                .filter(|column| column.relation() == table && matches!(column.source(), ColumnSource::CreateTable | ColumnSource::AddColumn))
                .collect::<Vec<_>>();
            let first_variable = columns
                .iter()
                .position(|column| schema.storage(column.data_type()) == Some(Storage::VariableLength));
            let Some(after) = first_variable.and_then(|position| columns.get(position)) else {
                return Vec::new();
            };
            columns
                .iter()
                .skip(first_variable.unwrap_or_default())
                .filter(|column| schema.storage(column.data_type()) == Some(Storage::FixedWidth))
                .filter(|column| !WALKED_COLUMNS.contains(&(table, column.name())))
                .map(|column| {
                    context.finding(
                        Rule::FixedWidthBeforeVariable,
                        column.origin(),
                        column.name().len(),
                        &LateFixedColumn {
                            table,
                            column: column.name(),
                            after: after.name(),
                        },
                    )
                })
                .collect()
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

    const FILE: &str = "branch/0001_events.sql";

    #[test]
    fn fixed_width_columns_before_variable_ones_are_accepted() -> Result<(), FixtureError> {
        let sql = "create table events (\n    event_key bigint not null,\n    occurred_at timestamptz not null,\n    entity_id text compression lz4 collate \"C\"\n);\n";
        assert_eq!(findings(Rule::FixedWidthBeforeVariable, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_fixed_width_column_after_a_variable_one_is_refused_at_its_name() -> Result<(), FixtureError> {
        let sql = "create table events (\n    event_key bigint not null,\n    entity_id text compression lz4 collate \"C\",\n    occurred_at timestamptz not null\n);\n";
        assert_eq!(
            findings(Rule::FixedWidthBeforeVariable, &[(FILE, sql)])?,
            ["migrations/branch/0001_events.sql:4:5"]
        );
        Ok(())
    }

    #[test]
    fn a_listed_column_a_check_leaves_behind_a_null_is_accepted_after_a_variable_one() -> Result<(), FixtureError> {
        let created = (
            "branch/0001_deliveries.sql",
            "create table notification_deliveries (\n    delivery_key bigint not null,\n    last_error text compression lz4 collate \"C\"\n);\n",
        );
        let added = (
            "branch/0002_delivery_change_order.sql",
            "alter table notification_deliveries\n    add column committed_at timestamptz not null default '-infinity',\n    add column event_ordinal integer not null default 0,\n    add column queued_at timestamptz;\n",
        );
        assert_eq!(
            findings(Rule::FixedWidthBeforeVariable, &[created, added])?,
            ["migrations/branch/0002_delivery_change_order.sql:4:16"]
        );
        Ok(())
    }

    #[test]
    fn a_composite_type_is_not_held_to_the_order() -> Result<(), FixtureError> {
        let sql = "create type entry as (\n    label text,\n    seen_at timestamptz\n);\n";
        assert_eq!(findings(Rule::FixedWidthBeforeVariable, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }
}
