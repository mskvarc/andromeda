//! The naming and storage conventions of `docs/database-schema-conventions.md`,
//! checked against every object the migrations declare.
//!
//! Every mechanically checkable rule runs over the declared schema, and any
//! violation fails: the schema carries no deviation from these rules.

use crate::migration_set::declared_schema::{ConstraintKind, DeclaredColumn, DeclaredConstraint, DeclaredIndex, DeclaredSchema, IndexKind};
use std::collections::BTreeSet;

/// A convention an object name or declaration breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rule {
    /// A constraint, index or statistics name does not start with the prefix
    /// its kind takes followed by its own table's name or abbreviation.
    ObjectPrefix,
    /// A name contains an abbreviation or a storage-type word it may not.
    ForbiddenWord,
    /// A variable-length column does not declare `COMPRESSION lz4`.
    MissingCompression,
    /// A size-bound check is named `_bound` rather than `_length`.
    BoundSuffix,
    /// An index serving a queue claim is not named `_claim`.
    ClaimIndexSuffix,
    /// A unique constraint over a write identity is not named `_write_identity`.
    WriteIdentitySuffix,
    /// A database-generated identity column is not named `_key`.
    IdentityNotKey,
    /// An integer column named `id` or `_id` holds a database-generated key.
    IntegerIdentifierNotKey,
    /// An integer array named `_ids` holds database-generated keys.
    IdentifierArrayNotKeys,
    /// A foreign-key column is not named like the column it references.
    ForeignKeyColumnName,
    /// A boolean column that is not a specification member does not read as a
    /// predicate.
    BooleanNotPredicate,
}

/// The constraint and index abbreviations each table may use in place of its
/// name.
const TABLE_ABBREVIATIONS: &[(&str, &str)] = &[
    ("current_attribute_values", "cav"),
    ("context_source_registrations", "csr"),
    ("pending_side_effects", "pse"),
];

/// The registered abbreviations, which may never appear in a table or column
/// name.
const ABBREVIATIONS: &[&str] = &["cav", "csr", "pse", "tea"];

/// Abbreviations no name may use at all.
const UNREGISTERED_ABBREVIATIONS: &[&str] = &["attr", "db"];

/// Storage-type words no name may carry.
const STORAGE_TYPE_WORDS: &[&str] = &["uuid", "geo", "bool", "datetime"];

/// Columns that hold a specification member under the member's own name, which
/// the word and boolean rules do not apply to.
const SPECIFICATION_MEMBER_COLUMNS: &[(&str, &str)] = &[("subscriptions", "geo_q"), ("subscriptions", "local_only"), ("subscriptions", "split_entities")];

/// Integer identifiers whose values a catalog assigns outside the database, so
/// they are `id`s rather than keys.
const CATALOG_ASSIGNED_IDENTIFIERS: &[(&str, &str)] = &[
    ("unit_codes", "id"),
    ("attribute_history", "unit_code_id"),
    ("current_attribute_values", "unit_code_id"),
];

/// One broken convention.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Violation {
    /// The rule broken.
    rule: Rule,
    /// The table, `table.column` or object name concerned.
    subject: String,
}

impl Violation {
    /// A violation of `rule` by `subject`.
    fn new(rule: Rule, subject: impl Into<String>) -> Violation {
        Violation { rule, subject: subject.into() }
    }
}

/// Whether `table.column` is listed in `list`.
fn is_listed(list: &[(&str, &str)], table: &str, column: &str) -> bool {
    list.iter()
        .any(|(listed_table, listed_column)| *listed_table == table && *listed_column == column)
}

/// The `_`-separated words of `name`.
fn words(name: &str) -> impl Iterator<Item = &str> {
    name.split('_')
}

/// The words of `name` no name may carry, plus the registered abbreviations
/// when `name` is a table or column name.
fn forbidden_words(name: &str, abbreviations_forbidden: bool) -> bool {
    words(name).any(|word| {
        UNREGISTERED_ABBREVIATIONS.contains(&word) || STORAGE_TYPE_WORDS.contains(&word) || (abbreviations_forbidden && ABBREVIATIONS.contains(&word))
    })
}

/// Whether `name` is `<prefix>_<table or its abbreviation>` or continues with
/// `_` after it.
fn has_table_prefix(name: &str, prefix: &str, table: &str) -> bool {
    let abbreviation = TABLE_ABBREVIATIONS
        .iter()
        .find(|(abbreviated, _)| *abbreviated == table)
        .map(|(_, abbreviation)| *abbreviation);
    [Some(table), abbreviation].into_iter().flatten().any(|table_part| {
        name.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('_'))
            .and_then(|rest| rest.strip_prefix(table_part))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
    })
}

/// The violations of the table-name rules.
fn table_violations(table: &str) -> Vec<Violation> {
    forbidden_words(table, true)
        .then(|| Violation::new(Rule::ForbiddenWord, table))
        .into_iter()
        .collect()
}

/// The violations of the column rules.
fn column_violations(column: &DeclaredColumn) -> Vec<Violation> {
    let subject = format!("{}.{}", column.table, column.name);
    let specification_member = is_listed(SPECIFICATION_MEMBER_COLUMNS, &column.table, &column.name);
    let type_word = column.type_word();
    let integer = ["SMALLINT", "INTEGER", "BIGINT"].contains(&type_word.as_str());
    let named_as_identifier = column.name == "id" || column.name.ends_with("_id");
    let checks = [
        (!specification_member && forbidden_words(&column.name, true), Rule::ForbiddenWord),
        (
            column.is_variable_length() && !column.definition.contains("COMPRESSION lz4"),
            Rule::MissingCompression,
        ),
        (column.is_identity() && !column.name.ends_with("_key"), Rule::IdentityNotKey),
        (
            integer && named_as_identifier && !column.is_identity() && !is_listed(CATALOG_ASSIGNED_IDENTIFIERS, &column.table, &column.name),
            Rule::IntegerIdentifierNotKey,
        ),
        (
            ["INTEGER[]", "BIGINT[]"].contains(&type_word.as_str()) && column.name.ends_with("_ids"),
            Rule::IdentifierArrayNotKeys,
        ),
        (
            type_word == "BOOLEAN"
                && !specification_member
                && column.name != "singleton"
                && !column.name.starts_with("value_")
                && !column.name.starts_with("is_"),
            Rule::BooleanNotPredicate,
        ),
    ];
    checks
        .into_iter()
        .filter(|(broken, _)| *broken)
        .map(|(_, rule)| Violation::new(rule, subject.as_str()))
        .collect()
}

/// The violations of the constraint rules.
fn constraint_violations(constraint: &DeclaredConstraint) -> Vec<Violation> {
    let prefix = match constraint.kind {
        ConstraintKind::PrimaryKey => "pk",
        ConstraintKind::Unique => "uq",
        ConstraintKind::ForeignKey => "fk",
        ConstraintKind::Check => "chk",
    };
    let foreign_key_misnamed = constraint.kind == ConstraintKind::ForeignKey
        && constraint
            .columns
            .iter()
            .zip(&constraint.referenced_columns)
            .any(|(column, referenced)| column != referenced && !(referenced == "id" && column.ends_with("_id")));
    let checks = [
        (!has_table_prefix(&constraint.name, prefix, &constraint.table), Rule::ObjectPrefix),
        (forbidden_words(&constraint.name, false), Rule::ForbiddenWord),
        (constraint.name.ends_with("_bound"), Rule::BoundSuffix),
        (
            constraint.kind == ConstraintKind::Unique
                && constraint.columns.iter().any(|column| column == "write_id")
                && !constraint.name.ends_with("_write_identity"),
            Rule::WriteIdentitySuffix,
        ),
        (foreign_key_misnamed, Rule::ForeignKeyColumnName),
    ];
    checks
        .into_iter()
        .filter(|(broken, _)| *broken)
        .map(|(_, rule)| Violation::new(rule, constraint.name.as_str()))
        .collect()
}

/// The violations of the index and statistics rules.
fn index_violations(index: &DeclaredIndex) -> Vec<Violation> {
    let prefix = match index.kind {
        IndexKind::Plain => "idx",
        IndexKind::Unique => "uq",
        IndexKind::Statistics => "stx",
    };
    let checks = [
        (!has_table_prefix(&index.name, prefix, &index.table), Rule::ObjectPrefix),
        (forbidden_words(&index.name, false), Rule::ForbiddenWord),
        (
            words(&index.name).any(|word| word.starts_with("claim")) && !index.name.ends_with("_claim"),
            Rule::ClaimIndexSuffix,
        ),
    ];
    checks
        .into_iter()
        .filter(|(broken, _)| *broken)
        .map(|(_, rule)| Violation::new(rule, index.name.as_str()))
        .collect()
}

/// Every violation of every rule in the embedded migrations.
fn every_violation() -> BTreeSet<Violation> {
    let schema = DeclaredSchema::of_every_migration();
    schema
        .tables
        .iter()
        .flat_map(|table| table_violations(table))
        .chain(schema.columns.iter().flat_map(column_violations))
        .chain(schema.constraints.iter().flat_map(constraint_violations))
        .chain(schema.indexes.iter().flat_map(index_violations))
        .collect()
}

#[test]
fn the_schema_reader_finds_every_kind_of_declared_object() {
    let schema = DeclaredSchema::of_every_migration();
    assert!(schema.tables.iter().any(|table| table == "current_attribute_values"));
    assert!(schema.tables.iter().any(|table| table == "tenants"), "platform tables are read");
    assert!(
        schema
            .columns
            .iter()
            .any(|column| column.table == "postgresql_maintenance_targets" && column.name == "last_decision"),
        "columns an ALTER TABLE adds are read"
    );
    assert!(
        schema
            .constraints
            .iter()
            .any(|constraint| constraint.kind == ConstraintKind::ForeignKey && constraint.table == "current_attribute_values"),
        "constraints an ALTER TABLE adds are read"
    );
    assert!(schema.indexes.iter().any(|index| index.kind == IndexKind::Unique));
    assert!(schema.indexes.iter().any(|index| index.kind == IndexKind::Statistics));
}

#[test]
fn no_migration_breaks_a_naming_or_storage_convention() {
    let violations = every_violation();
    assert!(violations.is_empty(), "these break docs/database-schema-conventions.md: {violations:#?}");
}

#[test]
fn a_table_part_is_the_table_name_or_its_registered_abbreviation() {
    assert!(has_table_prefix("pk_tenants", "pk", "tenants"));
    assert!(has_table_prefix("idx_cav_value_number", "idx", "current_attribute_values"));
    assert!(has_table_prefix(
        "chk_context_source_registrations_id_length",
        "chk",
        "context_source_registrations"
    ));
    assert!(!has_table_prefix("chk_tenant_status", "chk", "tenants"));
    assert!(!has_table_prefix("chk_pse_status", "chk", "tenants"));
    assert!(!has_table_prefix("idx_tenants_x", "uq", "tenants"));
}

#[test]
fn abbreviations_are_forbidden_in_table_and_column_names_only() {
    assert!(forbidden_words("csr_delivery_state", true));
    assert!(!forbidden_words("chk_csr_mode", false));
    assert!(forbidden_words("idx_cav_attr_number", false));
    assert!(forbidden_words("value_geo", true));
    assert!(!forbidden_words("value_geometry", true));
}
