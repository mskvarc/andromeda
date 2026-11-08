//! `foreign-key-column-name`: a referencing column is named like the column it
//! references, alone or behind the role it plays (`<role>_<column>`, so
//! `<what>_id` for a reference to an `id`), so a join reads as one name on
//! both sides.

use crate::{
    declared_schema::constraint::ConstraintKind,
    rule::{Rule, RuleContext, RuleFinding},
};

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

/// Whether `column` carries the name of `referenced`, alone or behind a role:
/// `entity_key` and `parent_entity_key` both name a reference to `entity_key`.
fn names_reference(column: &str, referenced: &str) -> bool {
    column == referenced || column.strip_suffix(referenced).is_some_and(|role| role.len() > 1 && role.ends_with('_'))
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
                .filter(|(column, referenced)| !names_reference(column, referenced))
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

    #[test]
    fn a_reference_named_like_its_target_behind_a_role_is_accepted() -> Result<(), FixtureError> {
        let sql = "create table links (\n    parent_entity_key bigint,\n    child_entity_key bigint,\n    _entity_key bigint,\n    blob_entity_key bigint,\n    constraint fk_links_parent foreign key (parent_entity_key) references entities (entity_key),\n    constraint fk_links_child foreign key (child_entity_key) references entities (entity_key),\n    constraint fk_links_bare foreign key (_entity_key) references entities (entity_key),\n    constraint fk_links_blob foreign key (blob_entity_key) references blobs (entity)\n);\n";
        assert_eq!(
            findings(Rule::ForeignKeyColumnName, &[("branch/0001_links.sql", sql)])?,
            ["migrations/branch/0001_links.sql:8:5", "migrations/branch/0001_links.sql:9:5"]
        );
        Ok(())
    }
}
