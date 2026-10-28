//! `no-or-replace`: no object is created `or replace`.
//!
//! A migration runs once, against a schema the earlier migrations built, so
//! there is nothing of its own to replace; `or replace` would silently
//! overwrite an object of the same signature that something else created
//! first.

use crate::{
    declared_schema::token_sequence::token_runs,
    rule::{Rule, RuleContext, RuleFinding},
};
use pg_query::protobuf::Token;

/// The tokens that create an object over an existing one.
const OR_REPLACE: [Token; 3] = [Token::Create, Token::Or, Token::Replace];

/// A `create or replace`.
#[derive(Debug, derive_more::Display)]
#[display("create or replace overwrites whatever object of the same signature exists")]
struct OrReplace;

/// Every `create or replace`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .migrations()
        .iter()
        .flat_map(|migration| {
            token_runs(migration, &OR_REPLACE)
                .into_iter()
                .map(|run| context.finding(Rule::NoOrReplace, migration.origin(run.start), run.len(), &OrReplace))
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

    const FILE: &str = "tenant/0001_functions.sql";

    #[test]
    fn a_plain_create_function_is_accepted() -> Result<(), FixtureError> {
        let sql = "create function one ()\n    returns integer\n    language sql\n    return 1;\n";
        assert_eq!(findings(Rule::NoOrReplace, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn create_or_replace_is_refused_at_create() -> Result<(), FixtureError> {
        let sql = "-- One.\ncreate or replace function one ()\n    returns integer\n    language sql\n    return 1;\n";
        assert_eq!(findings(Rule::NoOrReplace, &[(FILE, sql)])?, ["migrations/tenant/0001_functions.sql:2:1"]);
        Ok(())
    }
}
