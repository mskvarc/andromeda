//! `integer-identifier-not-key`: an integer named `id` or `_id` is an
//! identifier something outside the database assigns; one the database
//! generates is a `_key`.

use crate::{
    declared_schema::column::ColumnSource,
    rule::{Rule, RuleContext, RuleFinding},
};

/// The integer identifiers a catalogue outside the database assigns, as
/// `(table, column)`.
const CATALOGUE_ASSIGNED_IDENTIFIERS: &[(&str, &str)] = &[("attribute_history", "unit_code_id"), ("current_attributes", "unit_code_id")];

/// The name of an identifier.
const IDENTIFIER: &str = "id";

/// The ending of an identifier's name.
const IDENTIFIER_SUFFIX: &str = "_id";

/// An integer named as an identifier no catalogue assigns.
#[derive(Debug, derive_more::Display)]
#[display("{table}.{column} is an integer named as an identifier, which a database-generated key is not")]
struct IntegerIdentifier<'schema> {
    /// The table.
    table: &'schema str,
    /// The column.
    column: &'schema str,
}

/// Every scalar integer named `id` or `_id` that is neither an identity nor a
/// catalogue-assigned identifier.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .columns()
        .iter()
        .filter(|column| matches!(column.source(), ColumnSource::CreateTable | ColumnSource::AddColumn))
        .filter(|column| schema.integer_width(column.data_type()).is_some())
        .filter(|column| column.name() == IDENTIFIER || column.name().ends_with(IDENTIFIER_SUFFIX))
        .filter(|column| !column.is_identity() && !CATALOGUE_ASSIGNED_IDENTIFIERS.contains(&(column.relation(), column.name())))
        .map(|column| {
            context.finding(
                Rule::IntegerIdentifierNotKey,
                column.origin(),
                column.name().len(),
                &IntegerIdentifier {
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
    fn a_catalogue_identifier_is_accepted_and_another_integer_id_refused() -> Result<(), FixtureError> {
        let sql = "create table current_attributes (\n    unit_code_id smallint,\n    owner_id bigint\n);\n";
        assert_eq!(
            findings(Rule::IntegerIdentifierNotKey, &[("branch/0001_current_attributes.sql", sql)])?,
            ["migrations/branch/0001_current_attributes.sql:3:5"]
        );
        Ok(())
    }
}
