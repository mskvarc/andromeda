//! `foreign-key-indexed`: the referencing columns of every foreign key lead
//! the table's primary key, a unique constraint or an index.
//!
//! A delete or a key update in the referenced table looks the referencing
//! rows up by those columns, and without an index led by them that is a scan
//! of the referencing table under the referenced row's lock. A reference into
//! an append-only dictionary needs no index: its rows are never deleted or
//! rekeyed, so the lookup never runs.

use crate::{
    declared_schema::{
        DeclaredSchema,
        constraint::{ConstraintKind, DeclaredConstraint},
    },
    rule::{Rule, RuleContext, RuleFinding},
};
use std::collections::BTreeSet;

/// The append-only dictionaries, whose rows a trigger keeps from ever being
/// deleted or updated.
const APPEND_ONLY_DICTIONARIES: &[&str] = &["attribute_names", "entity_type_names"];

/// A foreign key whose columns lead no key or index of its table.
#[derive(Debug, derive_more::Display)]
#[display("no key or index of {table} is led by ({columns}), the columns of {constraint}")]
struct UnindexedForeignKey<'schema> {
    /// The referencing table.
    table: &'schema str,
    /// Its referencing columns, as a list.
    columns: String,
    /// The constraint.
    constraint: &'schema str,
}

/// Every foreign key whose referencing columns lead no key or index.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .constraints()
        .iter()
        .filter(|constraint| constraint.kind() == ConstraintKind::ForeignKey && !constraint.columns().is_empty())
        .filter(|constraint| {
            !constraint
                .referenced_table()
                .is_some_and(|table| APPEND_ONLY_DICTIONARIES.contains(&table.name()))
        })
        .filter(|constraint| !is_led(schema, constraint))
        .map(|constraint| {
            context.finding(
                Rule::ForeignKeyIndexed,
                constraint.origin(),
                constraint.name().map_or(0, str::len),
                &UnindexedForeignKey {
                    table: constraint.table(),
                    columns: constraint.columns().join(", "),
                    constraint: constraint.name().unwrap_or("its foreign key"),
                },
            )
        })
        .collect()
}

/// Whether a key or an index of the foreign key's table starts with its
/// columns, in any order.
fn is_led(schema: &DeclaredSchema, foreign_key: &DeclaredConstraint) -> bool {
    let wanted = foreign_key.columns().iter().map(String::as_str).collect::<BTreeSet<_>>();
    let leads = |leading: Vec<&str>| leading.len() >= wanted.len() && leading[..wanted.len()].iter().copied().collect::<BTreeSet<_>>() == wanted;
    let keys = schema
        .constraints()
        .iter()
        .filter(|key| key.table() == foreign_key.table() && matches!(key.kind(), ConstraintKind::PrimaryKey | ConstraintKind::Unique))
        .any(|key| leads(key.columns().iter().map(String::as_str).collect()));
    keys || schema
        .indexes()
        .iter()
        .filter(|index| index.table() == foreign_key.table() && index.predicate().is_none())
        .any(|index| leads(index.elements().iter().map_while(|element| element.column()).collect()))
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_entities.sql";

    const ENTITIES: &str = "create table entities (\n    entity_key bigint not null,\n    constraint pk_entities primary key (entity_key)\n);\n\n";

    #[test]
    fn a_foreign_key_led_by_an_index_or_the_primary_key_is_accepted() -> Result<(), FixtureError> {
        let sql = format!(
            "{ENTITIES}create table claims (\n    entity_key bigint not null,\n    claim_ordinal integer not null,\n    constraint pk_claims primary key (entity_key, claim_ordinal),\n    constraint fk_claims_entity foreign key (entity_key) references entities (entity_key)\n);\n\ncreate table notes (\n    entity_key bigint not null,\n    constraint fk_notes_entity foreign key (entity_key) references entities (entity_key)\n);\n\ncreate index idx_notes_entity_key on notes (entity_key);\n"
        );
        assert_eq!(findings(Rule::ForeignKeyIndexed, &[(FILE, &sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_foreign_key_no_index_leads_is_refused_at_the_constraint() -> Result<(), FixtureError> {
        let sql = format!(
            "{ENTITIES}create table notes (\n    note_ordinal integer not null,\n    entity_key bigint not null,\n    constraint pk_notes primary key (note_ordinal, entity_key),\n    constraint fk_notes_entity foreign key (entity_key) references entities (entity_key)\n);\n"
        );
        assert_eq!(
            findings(Rule::ForeignKeyIndexed, &[(FILE, &sql)])?,
            ["migrations/branch/0001_entities.sql:10:5"]
        );
        Ok(())
    }

    #[test]
    fn a_reference_into_an_append_only_dictionary_needs_no_index() -> Result<(), FixtureError> {
        let sql = "create table attribute_names (\n    attribute_name_key integer not null\n);\n\ncreate table notes (\n    attribute_name_key integer,\n    constraint fk_notes_attribute_name foreign key (attribute_name_key) references attribute_names (attribute_name_key)\n);\n";
        assert_eq!(findings(Rule::ForeignKeyIndexed, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }
}
