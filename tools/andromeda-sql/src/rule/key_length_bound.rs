//! `key-length-bound`: every text column a B-tree keys carries a check that
//! bounds its byte length within the B-tree entry limit.
//!
//! PostgreSQL refuses an index entry past 2,704 bytes with SQLSTATE 54000 at
//! insert time, so a text key without a bound is a write that fails on a long
//! value, and a bound larger than the limit admits values the index then
//! refuses. A primary key and a unique constraint are B-tree indexes too.

use crate::{
    declared_schema::{DeclaredSchema, check_bound::bounds_of, constraint::ConstraintKind, origin::Origin},
    rule::{Rule, RuleContext, RuleFinding},
};

/// The largest entry a B-tree page of the default size holds, in bytes.
const BTREE_ENTRY_LIMIT: i64 = 2_704;

/// The access method of a B-tree index.
const BTREE: &str = "btree";

/// A text key without a byte-length bound that fits.
#[derive(Debug, derive_more::Display)]
#[display("{table}.{column} is a text key of {key} and no check bounds its octet_length within {BTREE_ENTRY_LIMIT} bytes")]
struct UnboundedKey<'schema> {
    /// The table.
    table: &'schema str,
    /// The column.
    column: &'schema str,
    /// The key or index.
    key: &'schema str,
}

/// Every text column a primary key, a unique constraint or a B-tree index
/// keys that no check bounds within the limit.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let constraint_keys = schema
        .constraints()
        .iter()
        .filter(|constraint| matches!(constraint.kind(), ConstraintKind::PrimaryKey | ConstraintKind::Unique))
        .flat_map(|constraint| {
            let key = constraint.name().unwrap_or("a key");
            constraint
                .columns()
                .iter()
                .map(move |column| (constraint.table(), column.as_str(), key, constraint.origin(), key.len()))
        });
    let index_keys = schema.indexes().iter().filter(|index| index.method() == BTREE).flat_map(|index| {
        index
            .elements()
            .iter()
            .filter_map(|element| element.column())
            .map(move |column| (index.table(), column, index.name(), index.origin(), index.name().len()))
    });
    constraint_keys
        .chain(index_keys)
        .filter(|(table, column, ..)| is_text(schema, table, column) && !is_bounded(schema, table, column))
        .map(|(table, column, key, origin, length): (&str, &str, &str, Origin, usize)| {
            context.finding(Rule::KeyLengthBound, origin, length, &UnboundedKey { table, column, key })
        })
        .collect()
}

/// Whether `table.column` is of a text type.
fn is_text(schema: &DeclaredSchema, table: &str, column: &str) -> bool {
    schema
        .columns()
        .iter()
        .rev()
        .find(|declared| declared.relation() == table && declared.name() == column)
        .is_some_and(|declared| schema.is_collatable(declared.data_type()) && !declared.data_type().is_array())
}

/// Whether a check of `table` bounds the byte length of `column` within the
/// B-tree entry limit.
fn is_bounded(schema: &DeclaredSchema, table: &str, column: &str) -> bool {
    schema
        .constraints()
        .iter()
        .filter(|constraint| constraint.kind() == ConstraintKind::Check && constraint.table() == table)
        .filter_map(|constraint| constraint.expression())
        .flat_map(bounds_of)
        .any(|bound| bound.column() == column && bound.byte_length_limit().is_some_and(|limit| limit <= BTREE_ENTRY_LIMIT))
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "platform/0001_tenants.sql";

    #[test]
    fn a_text_key_bounded_within_the_limit_is_accepted() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    name text collate \"C\" not null,\n    constraint uq_tenants_name unique (name),\n    constraint chk_tenants_name_length check (octet_length(name) <= 1005)\n);\n";
        assert_eq!(findings(Rule::KeyLengthBound, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn an_unbounded_text_key_and_one_bounded_past_the_limit_are_refused() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    name text collate \"C\" not null,\n    label text collate \"C\",\n    constraint uq_tenants_name unique (name),\n    constraint chk_tenants_label_length check (octet_length(label) <= 4096)\n);\n\ncreate index idx_tenants_label on tenants (label);\n";
        assert_eq!(
            findings(Rule::KeyLengthBound, &[(FILE, sql)])?,
            ["migrations/platform/0001_tenants.sql:4:5", "migrations/platform/0001_tenants.sql:8:14"]
        );
        Ok(())
    }

    #[test]
    fn the_rule_does_not_fire_on_a_key_of_another_type() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    tenant_key bigint not null,\n    constraint pk_tenants primary key (tenant_key)\n);\n";
        assert_eq!(findings(Rule::KeyLengthBound, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }
}
