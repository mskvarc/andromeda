//! `integer-width`: a 32-bit integer is one of the few values that can never
//! outgrow it, and a 16-bit one is bounded by a check.
//!
//! An `integer` column or composite type field is a key of a name dictionary
//! or a reference to one (dictionary keys are minted far below 2^31), an
//! ordinal of something an instance holds, a schema version, a server process
//! id, which PostgreSQL itself types `integer`, or a counter on the
//! bounded-counter list that a check keeps at zero or above. Every other
//! integer is a `bigint`. A `smallint` table column is a code or a small
//! count, so a check bounds it, unless it is a catalogue identifier assigned
//! outside the database; a composite type field takes no check, so its
//! `smallint`s are bounded where the table column they travel to is. An array
//! is judged by its element type.

use crate::{
    declared_schema::{
        DeclaredSchema,
        check_bound::{ColumnBound, bounds_of},
        column::{ColumnSource, DeclaredColumn},
        constraint::ConstraintKind,
        declared_type::IntegerWidth,
    },
    rule::{Rule, RuleContext, RuleFinding},
};

/// The `integer` names that are keys of a name dictionary, or references to
/// one: exactly these, or ending in one of [`DICTIONARY_KEY_SUFFIXES`].
const DICTIONARY_KEY_NAMES: &[&str] = &[
    "value_type_key",
    "value_datatype_key",
    "object_type_keys",
    "watched_attributes",
    "expand_values",
    "json_keys",
    "property_name_keys",
    "relationship_name_keys",
    "change_attribute_name_keys",
    "entity_type_name_keys",
    "attribute_name_keys",
    "attribute_instance_name_keys",
    "evolution_type_name_keys",
    "attribute_name_key",
    "entity_type_name_key",
];

/// The endings of an `integer` name that keys or references a name dictionary.
const DICTIONARY_KEY_SUFFIXES: &[&str] = &["_name_key", "_name_keys"];

/// The `integer` names that are ordinals: exactly these, or ending in
/// `_ordinal`.
const ORDINAL_NAMES: &[&str] = &["ordinal", "position", "parent"];

/// The ending of an ordinal's name.
const ORDINAL_SUFFIX: &str = "_ordinal";

/// The `integer` names that hold a schema version.
const SCHEMA_VERSION_NAMES: &[&str] = &["schema_version", "branch_schema_version"];

/// The `integer` names that hold a server process id, which PostgreSQL types
/// `integer` in `pg_backend_pid()` and `pg_stat_activity.pid` alike.
const SERVER_PROCESS_ID_NAMES: &[&str] = &["backend_pid"];

/// The `integer` counters that cannot outgrow 32 bits: each counts the
/// attempts or the samples of one bounded episode, and is reset when it ends.
const BOUNDED_COUNTERS: &[&str] = &["attempt_count", "consecutive_failures", "breach_samples", "autovacuum_saturated_passes"];

/// The `smallint` identifiers a catalogue outside the database assigns, which
/// the catalogue rather than a check bounds.
const CATALOGUE_IDENTIFIERS: &[&str] = &["unit_code_id"];

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum WidthViolation<'schema> {
    /// A bounded counter no check keeps at zero or above.
    #[display("{relation}.{column} is a bounded counter no check keeps at zero or above")]
    UncheckedCounter {
        /// The table.
        relation: &'schema str,
        /// The column.
        column: &'schema str,
    },
    /// An `integer` that can outgrow 32 bits.
    #[display("{relation}.{column} is integer but no dictionary key, ordinal, schema version, server process id or bounded counter")]
    Unbounded {
        /// The table or composite type.
        relation: &'schema str,
        /// The column.
        column: &'schema str,
    },
    /// A `smallint` no check bounds.
    #[display("{relation}.{column} is smallint and no check bounds it")]
    UncheckedSmallint {
        /// The table.
        relation: &'schema str,
        /// The column.
        column: &'schema str,
    },
}

/// Every `integer` that is no key, ordinal, version, process id or bounded
/// counter, and every `smallint` table column no check bounds.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .columns()
        .iter()
        .filter(|column| column.source() != ColumnSource::AlterColumnType)
        .filter_map(|column| {
            let (relation, name) = (column.relation(), column.name());
            let violation = match schema.resolve_integer_element(column.data_type())? {
                IntegerWidth::Integer if is_bounded_by_meaning(column.name()) => return None,
                IntegerWidth::Integer if BOUNDED_COUNTERS.contains(&column.name()) && is_kept_non_negative(schema, column) => return None,
                IntegerWidth::Integer if BOUNDED_COUNTERS.contains(&column.name()) => WidthViolation::UncheckedCounter { relation, column: name },
                IntegerWidth::Integer => WidthViolation::Unbounded { relation, column: name },
                IntegerWidth::Smallint
                    if column.source() == ColumnSource::CompositeType || CATALOGUE_IDENTIFIERS.contains(&column.name()) || is_bounded(schema, column) =>
                {
                    return None;
                }
                IntegerWidth::Smallint => WidthViolation::UncheckedSmallint { relation, column: name },
                IntegerWidth::Bigint => return None,
            };
            Some(context.finding(Rule::IntegerWidth, column.origin(), name.len(), &violation))
        })
        .collect()
}

/// Whether an integer named `name` is a dictionary key, an ordinal, a schema
/// version or a server process id.
fn is_bounded_by_meaning(name: &str) -> bool {
    DICTIONARY_KEY_NAMES.contains(&name)
        || DICTIONARY_KEY_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        || ORDINAL_NAMES.contains(&name)
        || name.ends_with(ORDINAL_SUFFIX)
        || SCHEMA_VERSION_NAMES.contains(&name)
        || SERVER_PROCESS_ID_NAMES.contains(&name)
}

/// Whether a check of the column's table bounds the column.
fn is_bounded(schema: &DeclaredSchema, column: &DeclaredColumn) -> bool {
    checks_of(schema, column).next().is_some()
}

/// Whether a check of the column's table keeps the column at zero or above.
fn is_kept_non_negative(schema: &DeclaredSchema, column: &DeclaredColumn) -> bool {
    checks_of(schema, column).any(|bound| bound.is_non_negative())
}

/// The bounds the checks of the column's table put on the column.
fn checks_of<'schema>(schema: &'schema DeclaredSchema, column: &'schema DeclaredColumn) -> impl Iterator<Item = ColumnBound> + 'schema {
    schema
        .constraints()
        .iter()
        .filter(move |constraint| constraint.kind() == ConstraintKind::Check && constraint.table() == column.relation())
        .filter_map(|constraint| constraint.expression())
        .flat_map(bounds_of)
        .filter(move |bound| bound.column() == column.name())
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_deliveries.sql";

    #[test]
    fn keys_ordinals_versions_and_bounded_counters_are_accepted() -> Result<(), FixtureError> {
        let sql = "create table deliveries (\n    attribute_name_key integer not null,\n    write_ordinal integer not null,\n    schema_version integer not null,\n    attempt_count integer not null,\n    property_name_keys integer[],\n    kind smallint not null,\n    unit_code_id smallint,\n    constraint chk_deliveries_attempt_count_non_negative check (attempt_count >= 0),\n    constraint chk_deliveries_kind check (kind between 1 and 3)\n);\n";
        assert_eq!(findings(Rule::IntegerWidth, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_server_process_id_is_accepted_as_the_integer_the_server_types_it() -> Result<(), FixtureError> {
        let sql = "create table copies (\n    backend_pid integer not null\n);\n";
        assert_eq!(findings(Rule::IntegerWidth, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn an_unbounded_integer_counter_is_refused_at_its_name() -> Result<(), FixtureError> {
        let sql = "create table deliveries (\n    times_sent integer not null default 0,\n    constraint chk_deliveries_times_sent_non_negative check (times_sent >= 0)\n);\n";
        assert_eq!(findings(Rule::IntegerWidth, &[(FILE, sql)])?, ["migrations/branch/0001_deliveries.sql:2:5"]);
        Ok(())
    }

    #[test]
    fn a_bounded_counter_without_its_non_negative_check_is_refused() -> Result<(), FixtureError> {
        let sql = "create table deliveries (\n    attempt_count integer not null\n);\n";
        assert_eq!(findings(Rule::IntegerWidth, &[(FILE, sql)])?, ["migrations/branch/0001_deliveries.sql:2:5"]);
        Ok(())
    }

    #[test]
    fn an_unchecked_smallint_column_is_refused_and_a_composite_field_is_not() -> Result<(), FixtureError> {
        let sql = "create type entry as (\n    entry_kind smallint\n);\n\ncreate table deliveries (\n    revision smallint not null\n);\n";
        assert_eq!(findings(Rule::IntegerWidth, &[(FILE, sql)])?, ["migrations/branch/0001_deliveries.sql:6:5"]);
        Ok(())
    }

    #[test]
    fn the_rule_does_not_fire_on_a_bigint() -> Result<(), FixtureError> {
        assert_eq!(
            findings(Rule::IntegerWidth, &[(FILE, "create table deliveries (\n    times_sent bigint not null\n);\n")])?,
            Vec::<String>::new()
        );
        Ok(())
    }
}
