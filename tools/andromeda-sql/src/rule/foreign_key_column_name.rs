//! `foreign-key-column-name`: a referencing column is named like the column it
//! references, or `<what>_id` for a reference to an `id`, so a join reads as
//! one name on both sides.

use crate::{
    declared_schema::constraint::ConstraintKind,
    rule::{Rule, RuleContext, RuleFinding},
};

/// The name of an identifier a reference may extend.
const IDENTIFIER: &str = "id";

/// The ending of a column referencing an `id`.
const IDENTIFIER_SUFFIX: &str = "_id";

/// A referencing column named unlike its target.
#[derive(Debug, derive_more::Display)]
#[display("{table}.{column} references {referenced} under another name")]
struct MisnamedReference<'schema> {
    /// The referencing table.
    table: &'schema str,
    /// The referencing column.
    column: &'schema str,
    /// The referenced column.
    referenced: &'schema str,
}

/// Every foreign-key column named unlike the column it references.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .constraints()
        .iter()
        .filter(|constraint| constraint.kind() == ConstraintKind::ForeignKey)
        .flat_map(|constraint| {
            constraint
                .columns()
                .iter()
                .zip(constraint.referenced_columns())
                .filter(|(column, referenced)| column != referenced && !(*referenced == IDENTIFIER && column.ends_with(IDENTIFIER_SUFFIX)))
                .map(move |(column, referenced)| {
                    context.finding(
                        Rule::ForeignKeyColumnName,
                        constraint.origin(),
                        constraint.name().map_or(0, str::len),
                        &MisnamedReference {
                            table: constraint.table(),
                            column,
                            referenced,
                        },
                    )
                })
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

    #[test]
    fn a_reference_named_like_its_target_is_accepted_and_another_refused() -> Result<(), FixtureError> {
        let sql = "create table notes (\n    entity_key bigint,\n    owner bigint,\n    subscription_id text,\n    constraint fk_notes_entity foreign key (entity_key) references entities (entity_key),\n    constraint fk_notes_owner foreign key (owner) references entities (entity_key),\n    constraint fk_notes_subscription foreign key (subscription_id) references subscriptions (id)\n);\n";
        assert_eq!(
            findings(Rule::ForeignKeyColumnName, &[("branch/0001_notes.sql", sql)])?,
            ["migrations/branch/0001_notes.sql:6:5"]
        );
        Ok(())
    }
}
