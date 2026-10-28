//! `identity-not-key`: a column the database generates as an identity is
//! named `_key`, the word for a surrogate the database assigns.

use crate::{
    declared_schema::column::ColumnSource,
    rule::{Rule, RuleContext, RuleFinding},
};

/// The ending of a surrogate key's name.
const KEY_SUFFIX: &str = "_key";

/// An identity column not named `_key`.
#[derive(Debug, derive_more::Display)]
#[display("{table}.{column} is an identity and does not end in _key")]
struct IdentityNamed<'schema> {
    /// The table.
    table: &'schema str,
    /// The column.
    column: &'schema str,
}

/// Every identity column whose name does not end in `_key`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .columns()
        .iter()
        .filter(|column| matches!(column.source(), ColumnSource::CreateTable | ColumnSource::AddColumn))
        .filter(|column| column.is_identity() && !column.name().ends_with(KEY_SUFFIX))
        .map(|column| {
            context.finding(
                Rule::IdentityNotKey,
                column.origin(),
                column.name().len(),
                &IdentityNamed {
                    table: column.relation(),
                    column: column.name(),
                },
            )
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
    fn an_identity_is_named_key() -> Result<(), FixtureError> {
        let sql = "create table events (\n    event_key bigint generated always as identity,\n    serial bigint generated always as identity\n);\n";
        assert_eq!(findings(Rule::IdentityNotKey, &[(FILE, sql)])?, ["migrations/branch/0001_events.sql:3:5"]);
        Ok(())
    }
}
