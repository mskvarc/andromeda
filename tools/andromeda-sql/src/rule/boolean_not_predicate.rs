//! `boolean-not-predicate`: a boolean column reads as a predicate, `is_<what>`,
//! unless it is a value column or keeps a specification member's name.

use crate::{
    declared_schema::{ResolvedType, column::ColumnSource, declared_type::BuiltinType},
    rule::{Rule, RuleContext, RuleFinding, naming_vocabulary::is_specification_member},
};

/// The start of a predicate's name.
const PREDICATE_PREFIX: &str = "is_";

/// The start of a value column's name, which holds a value of any type.
const VALUE_PREFIX: &str = "value_";

/// The boolean column a one-row table is keyed by.
const SINGLETON: &str = "singleton";

/// A boolean that does not read as a predicate.
#[derive(Debug, derive_more::Display)]
#[display("{table}.{column} is a boolean that does not read as a predicate")]
struct NonPredicate<'schema> {
    /// The table.
    table: &'schema str,
    /// The column.
    column: &'schema str,
}

/// Every boolean table column not named `is_`, `value_` or as a
/// specification member.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .columns()
        .iter()
        .filter(|column| matches!(column.source(), ColumnSource::CreateTable | ColumnSource::AddColumn))
        .filter(|column| {
            schema.resolve(column.data_type())
                == ResolvedType::Builtin {
                    element: BuiltinType::Bool,
                    array: false,
                }
        })
        .filter(|column| {
            let name = column.name();
            name != SINGLETON && !name.starts_with(VALUE_PREFIX) && !name.starts_with(PREDICATE_PREFIX) && !is_specification_member(column.relation(), name)
        })
        .map(|column| {
            context.finding(
                Rule::BooleanNotPredicate,
                column.origin(),
                column.name().len(),
                &NonPredicate {
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
    fn a_predicate_and_a_specification_member_are_accepted_and_a_flag_refused() -> Result<(), FixtureError> {
        let sql = "create table subscriptions (\n    is_active boolean,\n    local_only boolean,\n    paused boolean\n);\n";
        assert_eq!(
            findings(Rule::BooleanNotPredicate, &[("branch/0001_subscriptions.sql", sql)])?,
            ["migrations/branch/0001_subscriptions.sql:4:5"]
        );
        Ok(())
    }
}
