//! `identifier-array-not-keys`: an array of database-generated integers is
//! named `_keys`, never `_ids`.

use crate::{
    declared_schema::{column::ColumnSource, declared_type::IntegerWidth},
    rule::{Rule, RuleContext, RuleFinding},
};

/// The ending of an identifier array's name.
const IDENTIFIERS_SUFFIX: &str = "_ids";

/// An integer array named `_ids`.
#[derive(Debug, derive_more::Display)]
#[display("{table}.{column} is an array of integer keys named _ids")]
struct IdentifierArray<'schema> {
    /// The table.
    table: &'schema str,
    /// The column.
    column: &'schema str,
}

/// Every `integer[]` or `bigint[]` column named `_ids`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .columns()
        .iter()
        .filter(|column| matches!(column.source(), ColumnSource::CreateTable | ColumnSource::AddColumn))
        .filter(|column| {
            column.data_type().is_array()
                && matches!(
                    schema.resolve_integer_element(column.data_type()),
                    Some(IntegerWidth::Integer | IntegerWidth::Bigint)
                )
                && column.name().ends_with(IDENTIFIERS_SUFFIX)
        })
        .map(|column| {
            context.finding(
                Rule::IdentifierArrayNotKeys,
                column.origin(),
                column.name().len(),
                &IdentifierArray {
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

    #[test]
    fn a_key_array_is_named_keys() -> Result<(), FixtureError> {
        let sql = "create table members (\n    entity_keys bigint[],\n    entity_ids bigint[]\n);\n";
        assert_eq!(
            findings(Rule::IdentifierArrayNotKeys, &[("branch/0001_members.sql", sql)])?,
            ["migrations/branch/0001_members.sql:3:5"]
        );
        Ok(())
    }
}
