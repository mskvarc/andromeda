//! `hypertable-shape`: a table converted into a hypertable has no foreign key
//! in either direction and no unique key without its partition column.
//!
//! TimescaleDB refuses a foreign key into a hypertable and a unique index
//! that does not include the partition column, and a foreign key out of one
//! is checked chunk by chunk and blocks dropping a chunk, so retention would
//! stall on it.

use crate::{
    declared_schema::{constraint::ConstraintKind, table::Hypertable},
    rule::{Rule, RuleContext, RuleFinding},
};

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum HypertableViolation<'schema> {
    /// The hypertable references another table.
    #[display("the hypertable {_0} declares a foreign key")]
    ReferencesOut(&'schema str),
    /// Another table references the hypertable.
    #[display("{referencing} references the hypertable {hypertable}")]
    ReferencedIn {
        /// The referencing table.
        referencing: &'schema str,
        /// The hypertable.
        hypertable: &'schema str,
    },
    /// A unique key of the hypertable omits its partition column.
    #[display("a unique key of the hypertable {_0} omits its partition column")]
    UnpartitionedKey(&'schema str),
}

/// Every foreign key into or out of a hypertable, and every unique key of one
/// without its partition column.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .hypertables()
        .iter()
        .flat_map(|hypertable: &Hypertable| {
            let table = hypertable.table();
            let partitioned = |columns: &[&str]| hypertable.partition_column().is_none_or(|partition| columns.contains(&partition));
            let constraints = schema.constraints().iter().filter_map(move |constraint| {
                let violation = match constraint.kind() {
                    ConstraintKind::ForeignKey if constraint.table() == table => HypertableViolation::ReferencesOut(table),
                    ConstraintKind::ForeignKey if constraint.referenced_table().is_some_and(|referenced| referenced.name() == table) => {
                        HypertableViolation::ReferencedIn {
                            referencing: constraint.table(),
                            hypertable: table,
                        }
                    }
                    ConstraintKind::PrimaryKey | ConstraintKind::Unique
                        if constraint.table() == table && !partitioned(&constraint.columns().iter().map(String::as_str).collect::<Vec<_>>()) =>
                    {
                        HypertableViolation::UnpartitionedKey(table)
                    }
                    ConstraintKind::ForeignKey | ConstraintKind::PrimaryKey | ConstraintKind::Unique | ConstraintKind::Check | ConstraintKind::Exclusion => {
                        return None;
                    }
                };
                Some(context.finding(Rule::HypertableShape, constraint.origin(), constraint.name().map_or(0, str::len), &violation))
            });
            let indexes = schema
                .indexes()
                .iter()
                .filter(move |index| index.table() == table && index.is_unique())
                .filter(move |index| !partitioned(&index.elements().iter().filter_map(|element| element.column()).collect::<Vec<_>>()))
                .map(move |index| {
                    context.finding(
                        Rule::HypertableShape,
                        index.origin(),
                        index.name().len(),
                        &HypertableViolation::UnpartitionedKey(table),
                    )
                });
            constraints.chain(indexes).collect::<Vec<_>>()
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

    const FILE: &str = "tenant/0001_history.sql";

    const CONVERSION: &str = "select\n    create_hypertable('history', by_range('recorded_at', interval '7 days'), create_default_indexes => false);\n";

    #[test]
    fn a_hypertable_keyed_with_its_partition_column_is_accepted() -> Result<(), FixtureError> {
        let sql = format!(
            "create table history (\n    stream_key bigint not null,\n    recorded_at timestamptz not null,\n    constraint uq_history_stream unique (stream_key, recorded_at)\n);\n\n{CONVERSION}"
        );
        assert_eq!(findings(Rule::HypertableShape, &[(FILE, &sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_unique_key_without_the_partition_column_and_a_reference_into_it_are_refused() -> Result<(), FixtureError> {
        let sql = format!(
            "create table history (\n    stream_key bigint not null,\n    recorded_at timestamptz not null,\n    constraint uq_history_stream unique (stream_key)\n);\n\ncreate table notes (\n    stream_key bigint,\n    constraint fk_notes_history foreign key (stream_key) references history (stream_key)\n);\n\n{CONVERSION}"
        );
        assert_eq!(
            findings(Rule::HypertableShape, &[(FILE, &sql)])?,
            ["migrations/tenant/0001_history.sql:4:5", "migrations/tenant/0001_history.sql:9:5"]
        );
        Ok(())
    }
}
